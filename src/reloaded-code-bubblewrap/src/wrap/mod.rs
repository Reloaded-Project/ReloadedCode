//! Wraps shell commands inside `bwrap` sandboxes.
//!
//! [`wrap_command`] builds a `bwrap` command from a validated
//! [`crate::profile::Profile`].
//! The `blocking` and `tokio` submodules adapt it for sync or async execution
//! via `process-wrap`.
//!
//! # Public API
//!
//! - [`wrap_command`] - build the wrapped command
//! - [`LinuxBwrapWrappedCommand`] - program path plus argv iterator
//!
//! # Feature Flags
//!
//! - `blocking` - enables the `blocking` submodule (sync)
//! - `tokio` - enables the `tokio` submodule (async)

pub(crate) mod command;

#[cfg(feature = "blocking")]
pub mod blocking;
#[cfg(feature = "tokio")]
pub mod tokio;

pub use crate::LinuxBwrapError;
pub use command::{wrap_command, LinuxBwrapWrappedCommand};
