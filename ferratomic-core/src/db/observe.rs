//! Observer registration for [`Database<Ready>`].
//!
//! INV-FERR-011: observers catch up to the current store before live delivery.

use ferratom::FerraError;

use super::{Database, Ready};
use crate::observer::DatomObserver;

impl Database<Ready> {
    /// Register a push-based datom observer.
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
}
