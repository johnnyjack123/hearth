//! Egress gateway of llm-harness instances (ARCHITECTURE.md §3.5).
//!
//! Instances live in internal networks; this HTTP proxy (`CONNECT` and plain
//! HTTP) is their only way out. Per target host it decides by the project's
//! rules, by approvals the user gave at runtime, or by asking the user while
//! the request waits. It resolves names itself and connects only to public
//! addresses, so the Windows host, the LAN, the tailnet and the VM stay
//! unreachable even in "open" mode (phase 0, sections 6.4 and 12.6).
//!
//! The host controls a running gateway through `harness-gateway ctl` inside
//! the gateway container (`podman exec`), which talks to a Unix socket that
//! instances cannot reach. Protocol: [`control`].
//!
//! Standard library only, so the image build needs no crates download, and
//! one thread per connection, which is plenty for one agent per gateway.

#![warn(missing_docs)]

pub mod address;
pub mod control;
mod gateway;
pub mod http;
pub mod pattern;
pub mod policy;
pub mod proxy;

pub use gateway::{Gateway, Options};
pub use pattern::{DomainPattern, Host};
pub use policy::{Mode, Policy, Reason};
