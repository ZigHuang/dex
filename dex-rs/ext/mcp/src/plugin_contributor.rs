//! Coordinates cloud turn refreshes and executor MCP contributions with thread-owned catalogs.
//!
//! One contributor is registered for both lifecycles. Provider instances are shared, while
//! catalogs and executor metadata live together in thread extension data.

use crate::PluginProviders;
use dex_core::config::Config;
use dex_core_plugins::ExecutorPluginProvider;
use dex_exec_server::EnvironmentManager;
use dex_extension_api::ExtensionRegistryBuilder;
use std::sync::Arc;

pub(crate) struct PluginContributor {
    pub(crate) providers: PluginProviders,
}

/// Installs one coordinator for cloud turn-start and executor MCP callbacks.
/// The optional cloud provider never disables executor plugins, and Core's plugin policy applies
/// to both paths.
pub fn install_plugin_providers(
    builder: &mut ExtensionRegistryBuilder<Config>,
    providers: PluginProviders,
) {
    let contributor = Arc::new(PluginContributor { providers });
    builder.thread_lifecycle_contributor(contributor.clone());
    builder.config_contributor(contributor.clone());
    builder.turn_lifecycle_contributor(contributor.clone());
    builder.mcp_server_contributor(contributor);
}

/// Installs plugin support backed by selected environment roots.
pub fn install_plugins(
    builder: &mut ExtensionRegistryBuilder<Config>,
    environment_manager: Arc<EnvironmentManager>,
) {
    install_plugin_providers(
        builder,
        PluginProviders::new(Arc::new(ExecutorPluginProvider::new(environment_manager))),
    );
}
