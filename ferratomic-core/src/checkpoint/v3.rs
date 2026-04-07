//! Checkpoint V3: thin facade over `ferratomic_checkpoint::v3`.
//!
//! The actual V3 serialization logic lives in the `ferratomic-checkpoint` crate.
//! This module is retained for source tree compatibility. All V3 functionality
//! is accessed through the ferratomic-checkpoint crate via the parent
//! checkpoint module's delegation.
