//! Resolve named credentials for model providers.
//!
//! Use [`CredentialResolver`] to look up credentials by their known names.
//! Callers can set explicit overrides and optionally fall back to process
//! environment variables.
//!
//! # Public API
//!
//! - [`CredentialResolver`] - Resolves one credential name at a time.
//! - [`CredentialLookup`] - Trait for abstracting credential resolution.
//!
//! # Precedence
//!
//! 1. Explicit overrides added with [`CredentialResolver::set_override`]
//! 2. Process environment variables, unless `READ_ENV` is `false`

use ahash::AHashMap;

/// Trait for resolving named credentials.
///
/// Implemented by [`CredentialResolver`] regardless of its `READ_ENV` parameter.
/// Use `&impl CredentialLookup` in function signatures to accept any resolver variant.
pub trait CredentialLookup {
    /// Resolves one named credential.
    ///
    /// Returns `None` when neither overrides nor environment variables provide a non-empty value.
    fn resolve(&self, name: &str) -> Option<String>;
}

/// Resolves named credentials from explicit overrides map.
#[derive(Debug, Clone)]
pub struct CredentialResolver<const READ_ENV: bool = true> {
    overrides: AHashMap<Box<str>, Box<str>>,
}

impl Default for CredentialResolver {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialResolver {
    /// Creates a resolver that checks overrides first and then falls back to environment variables.
    #[inline]
    pub fn new() -> Self {
        Self {
            overrides: AHashMap::new(),
        }
    }
}

impl CredentialResolver<false> {
    /// Creates a resolver that only uses explicit overrides (no environment fallback).
    #[inline]
    pub fn without_env() -> Self {
        Self {
            overrides: AHashMap::new(),
        }
    }
}

impl<const READ_ENV: bool> CredentialResolver<READ_ENV> {
    /// Stores a value to return when callers resolve `name`.
    #[inline]
    pub fn set_override(&mut self, name: impl Into<Box<str>>, value: impl Into<Box<str>>) {
        self.overrides.insert(name.into(), value.into());
    }
}

impl<const READ_ENV: bool> CredentialLookup for CredentialResolver<READ_ENV> {
    #[inline]
    fn resolve(&self, name: &str) -> Option<String> {
        if let Some(value) = self.overrides.get(name) {
            if !value.is_empty() {
                return Some(value.as_ref().to_owned());
            }
        }

        // Statically resolved: when READ_ENV is false, the compiler eliminates this branch entirely.
        if READ_ENV {
            if let Ok(value) = std::env::var(name) {
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::{CredentialLookup, CredentialResolver};

    #[test]
    fn resolve_prefers_overrides_to_environment_variables() {
        temp_env::with_var("OPENAI_API_KEY", Some("env-key"), || {
            let mut resolver = CredentialResolver::new();
            resolver.set_override("OPENAI_API_KEY", "override-key");

            assert_eq!(
                resolver.resolve("OPENAI_API_KEY").as_deref(),
                Some("override-key")
            );
        });
    }

    #[test]
    fn resolve_skips_empty_override_and_uses_environment_value() {
        temp_env::with_var("OPENAI_API_KEY", Some("env-key"), || {
            let mut resolver = CredentialResolver::new();
            resolver.set_override("OPENAI_API_KEY", "");

            assert_eq!(
                resolver.resolve("OPENAI_API_KEY").as_deref(),
                Some("env-key")
            );
        });
    }

    #[test]
    fn without_env_ignores_environment_variables() {
        temp_env::with_var("OPENAI_API_KEY", Some("env-key"), || {
            let mut resolver = CredentialResolver::without_env();
            resolver.set_override("OPENAI_API_KEY", "override-key");

            assert_eq!(
                resolver.resolve("OPENAI_API_KEY").as_deref(),
                Some("override-key")
            );
            assert_eq!(resolver.resolve("ANTHROPIC_API_KEY"), None);
        });
    }
}
