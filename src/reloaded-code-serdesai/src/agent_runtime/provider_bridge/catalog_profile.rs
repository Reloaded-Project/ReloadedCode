//! Catalog token limits layered over models built by the provider bridge.
//!
//! Wrapping a built model lets its [`ModelProfile`] report the catalog
//! entry's `max_input`/`max_output` as `context_window`/`max_tokens`, so
//! limit-aware features work on real providers. Catalog entries without
//! usable limits keep the built model's own profile.

use super::ResolvedSerdesModel;
use async_trait::async_trait;
use reloaded_code_agents::ResolvedModel;
use reloaded_code_core::models::ModelCatalog;
use serdes_ai::core::{ModelRequest, ModelResponse, ModelSettings};
use serdes_ai_models::{
    BoxedModel, Model as SerdesModel, ModelError, ModelProfile, ModelRequestParameters,
    StreamedResponse,
};
use std::sync::Arc;

/// Model that serves a catalog-populated profile while delegating requests.
///
/// Every operation forwards to `inner`; only [`SerdesModel::profile`]
/// changes, reporting the catalog entry's token limits instead of the
/// vendor preset. Profile-derived capability checks such as `supports` are
/// unaffected because only the two limit fields differ.
struct CatalogProfileModel {
    /// Wrapped provider model serving all requests.
    inner: BoxedModel,
    /// Inner profile with catalog token limits applied.
    profile: ModelProfile,
}

#[async_trait]
impl SerdesModel for CatalogProfileModel {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn system(&self) -> &str {
        self.inner.system()
    }

    fn profile(&self) -> &ModelProfile {
        &self.profile
    }

    async fn request(
        &self,
        messages: &[ModelRequest],
        settings: &ModelSettings,
        params: &ModelRequestParameters,
    ) -> Result<ModelResponse, ModelError> {
        self.inner.request(messages, settings, params).await
    }

    async fn request_stream(
        &self,
        messages: &[ModelRequest],
        settings: &ModelSettings,
        params: &ModelRequestParameters,
    ) -> Result<StreamedResponse, ModelError> {
        self.inner.request_stream(messages, settings, params).await
    }

    async fn count_tokens(&self, messages: &[ModelRequest]) -> Result<u64, ModelError> {
        self.inner.count_tokens(messages).await
    }
}

/// Populates a built model's profile limits from its catalog entry.
///
/// Non-zero `max_input`/`max_output` become [`ModelProfile::context_window`]
/// and [`ModelProfile::max_tokens`]. Missing entries, or entries whose
/// limits are all zero, return `built` unchanged so vendor defaults survive.
pub(super) fn with_catalog_limits(
    catalog: &ModelCatalog,
    resolved: &ResolvedModel,
    built: ResolvedSerdesModel,
) -> ResolvedSerdesModel {
    let Some(entry) = catalog.lookup_provider_model(resolved.provider(), resolved.model()) else {
        return built;
    };
    if entry.max_input == 0 && entry.max_output == 0 {
        return built;
    }

    // One-time cost at model construction; request paths only read the result.
    let mut profile = built.model.profile().clone();
    if entry.max_input > 0 {
        profile.context_window = Some(u64::from(entry.max_input));
    }
    if entry.max_output > 0 {
        profile.max_tokens = Some(u64::from(entry.max_output));
    }

    ResolvedSerdesModel {
        model: Arc::new(CatalogProfileModel {
            inner: built.model,
            profile,
        }),
        spec: built.spec,
    }
}
