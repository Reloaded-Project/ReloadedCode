//! Registries for storing and looking up custom tool factories.

use super::factory::ToolFactory;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

/// Registry of custom tool factories, keyed by tool name.
#[derive(Default)]
pub struct CustomToolRegistry {
    factories: HashMap<&'static str, Box<dyn ToolFactory>>,
}

impl std::fmt::Debug for CustomToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomToolRegistry")
            .field("factories", &self.factories.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl CustomToolRegistry {
    /// Creates an empty registry.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a custom tool factory, keyed by [`ToolContext::name`].
    ///
    /// If a factory with the same name already exists, it is replaced.
    ///
    /// [`ToolContext::name`]: crate::ToolContext::name
    pub fn insert(&mut self, factory: impl ToolFactory + 'static) {
        self.factories.insert(factory.name(), Box::new(factory));
    }

    /// Looks up a factory by tool name.
    ///
    /// Returns `None` when no factory is registered under `name`.
    #[inline]
    pub fn get(&self, name: &str) -> Option<&dyn ToolFactory> {
        self.factories.get(name).map(|f| f.as_ref())
    }

    /// Returns `true` if the registry contains no factories.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }

    /// Returns the number of registered factories.
    #[inline]
    pub fn len(&self) -> usize {
        self.factories.len()
    }
}

/// Shared wrapper around a [`CustomToolRegistry`], cheaply cloneable via [`Arc`].
///
/// Cloning shares the same underlying map, making it cheap to pass through
/// runtime builders and framework adapters.
#[derive(Debug, Clone)]
pub struct SharedToolRegistry {
    inner: Arc<CustomToolRegistry>,
}

impl SharedToolRegistry {
    /// Creates an empty registry.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CustomToolRegistry::new()),
        }
    }

    /// Creates a shared registry from a populated [`CustomToolRegistry`].
    #[inline]
    #[must_use]
    pub fn from_registry(registry: CustomToolRegistry) -> Self {
        Self {
            inner: Arc::new(registry),
        }
    }
}

impl Deref for SharedToolRegistry {
    type Target = CustomToolRegistry;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Default for SharedToolRegistry {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
