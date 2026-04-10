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
