//! Fixtures for integration tests that seed provider credentials and model caches.

use dex_login::DexAuth;
use dex_model_provider_info::ModelProviderInfo;
use dex_models_manager::cache::ModelsCacheEntry;
use dex_protocol::openai_models::ModelInfo;

/// Constructs a cache fixture matching the supplied test provider and authentication.
pub fn models_cache_entry(
    provider_info: &ModelProviderInfo,
    auth: Option<&DexAuth>,
    models: Vec<ModelInfo>,
) -> ModelsCacheEntry {
    ModelsCacheEntry {
        fetched_at: std::time::SystemTime::now().into(),
        etag: None,
        client_version: Some(dex_models_manager::client_version_to_whole()),
        identity: crate::models_identity::identity(provider_info, auth).ok(),
        models,
    }
}

pub use crate::shared_state::test_support::seed_gateway_auth;

pub use crate::provider::test_support::runtime_model_provider;
