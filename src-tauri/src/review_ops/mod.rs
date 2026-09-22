//! Verify execution ownership shared by Desktop and Runner adapters.

mod claims;
mod store;
pub mod verify;

pub(crate) use claims::ReviewClaim;
pub use claims::ReviewClaims;
