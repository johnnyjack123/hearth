//! Core of the `harness` sandbox: provider abstraction, the Podman provider and
//! the embedded base image. Shared by the CLI and (later) `harnessd`.
//!
//! Design notes live in `ARCHITECTURE.md` §3 and `docs/ROADMAP.md` (Phase 2).

#![warn(missing_docs)]

pub mod error;
pub mod image;
pub mod paths;
pub mod podman;
pub mod provider;

pub use error::{Error, Result};
pub use provider::Provider;

/// Label set on every resource the harness creates, so cleanup and status
/// never touch containers, volumes or networks the user made by hand.
pub const MANAGED_LABEL: &str = "io.llm-harness.managed";
