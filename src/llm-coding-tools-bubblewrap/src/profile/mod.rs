//! Bubblewrap profile types, builders, and preset helpers.
//!
//! Public callers should use the short aliases in this module:
//! - [`Builder`] - builds a sandbox profile
//! - [`Profile`] - validated sandbox profile ready for repeated execution
//! - [`Preset`] - preset name stored on the profile
//! - [`TmpBacking`] - how sandbox `/tmp` is mounted

mod builder;
pub(crate) mod layout;
mod presets;
mod types;
pub(crate) mod validation;

pub use builder::Builder;
pub use types::{
    Availability, EnvVar, FileMount, FileOverlay, NetworkPolicy, Preset, Profile, Symlink,
    TmpBacking,
};
