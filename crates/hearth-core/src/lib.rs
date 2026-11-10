//! Core of the `hearth` sandbox: provider abstraction, the Podman provider,
//! the embedded base image and the host configuration. Shared by the CLI and
//! (later) `hearthd`.
//!
//! Design notes live in `ARCHITECTURE.md` §3 and `docs/ROADMAP.md` (Phase 2).

#![warn(missing_docs)]

pub mod app_file;
pub mod apps;
pub mod briefing;
pub mod busy;
pub mod config;
pub mod error;
pub mod gateway;
pub mod image;
pub mod instance;
pub mod lang;
pub mod legacy;
pub mod osv;
pub mod paths;
pub mod pi;
pub mod pi_rpc;
pub mod podman;
pub mod project;
pub mod provider;
#[cfg(windows)]
pub mod pty;
pub mod rescue;
pub mod review;
pub mod server;
pub mod sync;
pub mod tunnel;
pub mod vmd;

pub use error::{Error, Result};
pub use provider::Provider;

/// Label set on every resource the harness creates, so cleanup and status
/// never touch containers, volumes or networks the user made by hand.
pub const MANAGED_LABEL: &str = "io.hearth.managed";

/// Label naming the project a container or network belongs to.
pub const PROJECT_LABEL: &str = "io.hearth.project";
