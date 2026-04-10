//! Observer registration for [`Database<Ready>`].
//!
//! INV-FERR-011: observers catch up to the current store before live delivery.

use ferratom::{DatomFilter, FerraError};

use super::{Database, Ready};
use crate::observer::DatomObserver;

impl Database<Ready> {
    /// Register a push-based datom observer (unfiltered, receives all datoms).
    ///
    /// INV-FERR-011: the observer is caught up to the current store state
    /// before future commit notifications are delivered.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if the observer registry
    /// mutex is poisoned.
    pub fn register_observer(&self, observer: Box<dyn DatomObserver>) -> Result<(), FerraError> {
        let mut observers = self
            .observers
            .lock()
            .map_err(|_| FerraError::InvariantViolation {
                invariant: "INV-FERR-011".to_string(),
                details: "observer registry mutex poisoned during register".to_string(),
            })?;
        let current = self.current.load();
        observers.register(observer, current.as_ref());
        Ok(())
    }

    /// Register a filtered observer (INV-FERR-044: namespace isolation).
    ///
    /// The observer receives only datoms matching the filter, both during
    /// catchup and live delivery. Epoch monotonicity (INV-FERR-011) is
    /// preserved — filtered empty batches are still delivered at the
    /// correct epoch.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if the observer registry
    /// mutex is poisoned.
    pub fn register_filtered_observer(
        &self,
        observer: Box<dyn DatomObserver>,
        filter: DatomFilter,
    ) -> Result<(), FerraError> {
        let mut observers = self
            .observers
            .lock()
            .map_err(|_| FerraError::InvariantViolation {
                invariant: "INV-FERR-011".to_string(),
                details: "observer registry mutex poisoned during register".to_string(),
            })?;
        let current = self.current.load();
        observers.register_filtered(observer, filter, current.as_ref());
        Ok(())
    }
}
