//! Error type for provider configuration operations.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur when loading or validating provider configuration.
#[derive(Debug, Error)]
pub enum ProviderConfigError {
    /// A config file could not be read.
    #[error("failed to read config file `{path}`: {source}")]
    FileRead {
        /// Path of the config file.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// A config file contains invalid YAML.
    #[error("failed to parse config file `{path}`: {source}")]
    YamlParse {
        /// Path of the config file.
        path: PathBuf,
        /// Underlying YAML parse error.
        source: serde_yaml::Error,
    },
    /// A provider entry is missing required fields.
    #[error("provider `{provider_key}` is missing required field: {field}")]
    MissingField {
        /// Provider key from the YAML map.
        provider_key: String,
        /// Name of the missing field.
        field: &'static str,
    },
    /// A model entry is missing required fields.
    #[error("provider `{provider_key}` model `{model_key}` is missing required field: {field}")]
    ModelMissingField {
        /// Provider key from the YAML map.
        provider_key: String,
        /// Model key from the provider's models map.
        model_key: String,
        /// Name of the missing field.
        field: &'static str,
    },
    /// A modality string is not recognized.
    #[error("provider `{provider_key}` model `{model_key}` has unrecognized modality `{value}`; expected one of: text, image, audio, video")]
    UnrecognizedModality {
        /// Provider key from the YAML map.
        provider_key: String,
        /// Model key from the provider's models map.
        model_key: String,
        /// The unrecognized modality string.
        value: String,
    },
    /// An `api_type` string is not recognized.
    #[error("provider `{provider_key}` has unrecognized api_type `{value}`")]
    UnrecognizedApiType {
        /// Provider key from the YAML map.
        provider_key: String,
        /// The unrecognized api_type string.
        value: String,
    },
    /// Provider count exceeds the `u16` provider-index address space.
    #[error("provider count {count} exceeds supported maximum {max}")]
    TooManyProviders {
        /// Number of providers supplied.
        count: usize,
        /// Maximum supported provider count.
        max: usize,
    },
}
