#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]

#[cfg(not(target_os = "linux"))]
compile_error!("reloaded-code-bubblewrap is only supported on Linux");

mod error;
mod path_util;
mod probe;
pub mod profile;
pub mod wrap;

#[cfg(test)]
mod test_helpers;

pub use error::LinuxBwrapError;
pub use profile::{
    create_sandbox, create_sandbox_with, create_temp_sandbox, CreateSandboxError, SandboxDirs,
    TempSandboxDirs,
};
pub use profile::{
    Availability, Builder, EnvVar, FileMount, NetworkPolicy, Preset, Profile, Symlink, TmpBacking,
};
pub use wrap::LinuxBwrapWrappedCommand;
