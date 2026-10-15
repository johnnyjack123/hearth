//! Core of the `harness` sandbox: provider abstraction, the Podman provider,
//! the embedded base image and the host configuration. Shared by the CLI and
//! (later) `harnessd`.
//!
//! Design notes live in `ARCHITECTURE.md` §3 and `docs/ROADMAP.md` (Phase 2).

#![warn(missing_docs)]

pub mod briefing;
pub mod config;
pub mod error;
pub mod gateway;
pub mod image;
pub mod instance;
pub mod paths;
pub mod pi;
pub mod podman;
pub mod project;
pub mod provider;
pub mod server;
pub mod sync;

pub use error::{Error, Result};
pub use provider::Provider;

/// Label set on every resource the harness creates, so cleanup and status
/// never touch containers, volumes or networks the user made by hand.
pub const MANAGED_LABEL: &str = "io.llm-harness.managed";

/// Label naming the project a container or network belongs to.
pub const PROJECT_LABEL: &str = "io.llm-harness.project";
