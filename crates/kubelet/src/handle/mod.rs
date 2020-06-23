//! A convenience handle type for providers
//!
//! A collection of handle types for use in providers. These are entirely
//! optional, but abstract away much of the logic around managing logging,
//! status updates, and stopping pods
mod stopper;

pub use stopper::StopHandler;
