//! Federation transport trait for peer-to-peer datom exchange.
//!
//! INV-FERR-038: Transport transparency — local and remote stores are
//! indistinguishable from the query perspective.
//!
//! ADR-FERR-024: Async via `std::future` only. Zero async runtime deps.
//! All methods return `Pin<Box<dyn Future>>` so the trait is dyn-compatible
//! and usable with any async executor (or `block_on` for sync callers).
//!
//! ADR-FERR-025: Transaction is the natural unit of federation.
//! `fetch_signed_transactions` returns `SignedTransactionBundle` per tx.

use std::{future::Future, pin::Pin, time::Duration};

use ferratom::{Datom, DatomFilter, FerraError, Frontier, Schema, SignedTransactionBundle};

/// Federation transport: fetch datoms and signed transactions from a peer.
///
/// INV-FERR-038: Transport transparency — consumers interact with local
/// and remote stores through the same interface. The implementation
/// determines whether data is fetched from local memory, a network peer,
/// or a content-addressed block store.
///
/// ADR-FERR-024: All methods return `Pin<Box<dyn Future + Send>>`.
/// No async runtime dependency — callers choose their executor.
///
/// Requires `Send + Sync` for cross-thread usage in async contexts.
pub trait Transport: Send + Sync {
    /// Fetch datoms matching the filter from the peer.
    ///
    /// INV-FERR-039: The filter is applied at the source, so only
    /// matching datoms are transferred.
    fn fetch_datoms<'a>(
        &'a self,
        filter: &'a DatomFilter,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Datom>, FerraError>> + Send + 'a>>;

    /// Fetch signed transaction bundles matching the filter.
    ///
    /// ADR-FERR-025: Transaction is the natural unit of federation.
    /// Each bundle contains user datoms, metadata, and signature.
    fn fetch_signed_transactions<'a>(
        &'a self,
        filter: &'a DatomFilter,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SignedTransactionBundle>, FerraError>> + Send + 'a>>;

    /// Fetch the peer's schema.
    fn schema<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Schema, FerraError>> + Send + 'a>>;

    /// Fetch the peer's causal frontier (INV-FERR-061).
    fn frontier<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Frontier, FerraError>> + Send + 'a>>;

    /// Ping the peer for liveness check. Returns round-trip latency.
    fn ping<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Duration, FerraError>> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// LocalTransport — in-process federation (INV-FERR-038)
// ---------------------------------------------------------------------------

use std::{collections::BTreeMap, sync::Arc};

use crate::db::{Database, Ready};

/// In-process transport backed by a local [`Database`].
///
/// INV-FERR-038: Transport transparency — `LocalTransport` implements the
/// same `Transport` trait as network transports, making local and remote
/// stores interchangeable in federation code.
///
/// All methods return immediately via `std::future::ready` (zero latency).
pub struct LocalTransport {
    db: Arc<Database<Ready>>,
}

impl LocalTransport {
    /// Wrap a database in a `LocalTransport`.
    #[must_use]
    pub fn new(db: Arc<Database<Ready>>) -> Self {
        Self { db }
    }
}

impl Transport for LocalTransport {
    fn fetch_datoms<'a>(
        &'a self,
        filter: &'a DatomFilter,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Datom>, FerraError>> + Send + 'a>> {
        let snap = self.db.snapshot();
        let datoms: Vec<Datom> = snap
            .datoms()
            .filter(|d| filter.matches(d))
            .cloned()
            .collect();
        Box::pin(std::future::ready(Ok(datoms)))
    }

    fn fetch_signed_transactions<'a>(
        &'a self,
        filter: &'a DatomFilter,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SignedTransactionBundle>, FerraError>> + Send + 'a>>
    {
        let snap = self.db.snapshot();
        let bundles = group_into_bundles(&snap, filter);
        Box::pin(std::future::ready(Ok(bundles)))
    }

    fn schema<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Schema, FerraError>> + Send + 'a>> {
        Box::pin(std::future::ready(Ok(self.db.schema())))
    }

    fn frontier<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Frontier, FerraError>> + Send + 'a>> {
        Box::pin(std::future::ready(self.db.frontier()))
    }

    fn ping<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Duration, FerraError>> + Send + 'a>> {
        Box::pin(std::future::ready(Ok(Duration::ZERO)))
    }
}

/// Group snapshot datoms into `SignedTransactionBundle` per `TxId`.
///
/// ADR-FERR-025: Transaction is the natural unit of federation.
/// Datoms are grouped by their `TxId`, then each group is converted
/// to a bundle via `SignedTransactionBundle::from_store_datoms`.
/// Group snapshot datoms into `SignedTransactionBundle` per `TxId`.
///
/// DEFECT-004 fix: first pass collects only USER datoms (non-tx/*) that
/// match the filter. Second pass collects ALL tx/* metadata for accepted
/// transaction IDs. This ensures `tx/*` metadata is always included for
/// context but never causes a transaction to be accepted just because
/// its metadata matched.
fn group_into_bundles(
    snap: &crate::store::Snapshot,
    filter: &DatomFilter,
) -> Vec<SignedTransactionBundle> {
    let mut accepted_txids: std::collections::BTreeSet<ferratom::TxId> =
        std::collections::BTreeSet::new();
    let mut groups: BTreeMap<ferratom::TxId, Vec<Datom>> = BTreeMap::new();

    // First pass: user datoms only (skip tx/* metadata).
    for datom in snap.datoms() {
        if !datom.attribute().as_str().starts_with("tx/") && filter.matches(datom) {
            accepted_txids.insert(datom.tx());
            groups.entry(datom.tx()).or_default().push(datom.clone());
        }
    }

    // Second pass: collect ALL datoms (including tx/* metadata) for accepted TxIds.
    for datom in snap.datoms() {
        if accepted_txids.contains(&datom.tx()) && datom.attribute().as_str().starts_with("tx/") {
            groups.entry(datom.tx()).or_default().push(datom.clone());
        }
    }

    groups
        .into_iter()
        .map(|(tx_id, datoms)| SignedTransactionBundle::from_store_datoms(&datoms, tx_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// INV-FERR-038: Transport trait must be dyn-compatible and Send + Sync.
    #[test]
    fn test_inv_ferr_038_transport_bounds() {
        // Compilation of these type assertions proves the trait bounds.
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn Transport>();
        assert_send_sync::<Box<dyn Transport>>();
    }
}
