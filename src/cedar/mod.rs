//! Cedar ABAC authorization integration for PDT
//!
//! This module wires PEP's Cedar authorizer into PDT's asset handlers.
//! Assets with an `auth_context` field go through Cedar evaluation;
//! assets without it (grandfathered) pass through with open access.

pub mod enforcement;
pub mod entity;

pub use enforcement::should_enforce_cedar;
