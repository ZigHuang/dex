use std::sync::Arc;

use dex_config::McpServerTransportConfig;
use dex_config::test_support::CloudConfigBundleFixture;
use dex_core::McpManager;
use dex_core::config::Config;
use dex_core::config::ConfigBuilder;
use dex_core::plugins_manager_for_config;
use dex_extension_api::ExtensionRegistryBuilder;
use dex_extension_api::McpServerContribution;
use dex_extension_api::McpServerContributionContext;
use dex_extension_api::McpServerContributor;
use dex_login::AuthManager;
use dex_login::DexAuth;
use dex_login::test_support::auth_manager_from_optional_auth;
use dex_mcp::DEX_APPS_MCP_SERVER_NAME;
use pretty_assertions::assert_eq;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn contributes_hosted_plugin_runtime_without_an_executor() -> TestResult {
    let dex_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .dex_home(dex_home.path().to_path_buf())
        .fallback_cwd(Some(dex_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            ("chatgpt_base_url".to_string(), "https://chatgpt.com".into()),
        ])
        .build()
        .await?;
    let auth = DexAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = installed_manager(&config, Some(auth.clone()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(DEX_APPS_MCP_SERVER_NAME)
        .ok_or("hosted plugin runtime should be contributed as a configured server")?
        .config();
    let McpServerTransportConfig::StreamableHttp { url, .. } = &server.transport else {
        panic!("hosted plugin runtime should use streamable HTTP");
    };
    assert_eq!(url, "https://chatgpt.com/backend-api/ps/mcp");

    Ok(())
}

#[tokio::test]
async fn runtime_overlay_preserves_disabled_server() -> TestResult {
    let dex_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .dex_home(dex_home.path().to_path_buf())
        .fallback_cwd(Some(dex_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            (
                "mcp_servers.dex_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
            ("mcp_servers.dex_apps.enabled".to_string(), false.into()),
        ])
        .cloud_config_bundle(
            CloudConfigBundleFixture::loader_with_enterprise_requirement("[mcp_servers]"),
        )
        .build()
        .await?;
    let auth = DexAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = installed_manager(&config, Some(auth.clone()));

    let configured = manager.configured_servers(&config).await;
    // Standalone login must still reject requirements-denied, explicitly disabled entries.
    let login_client = Arc::new(dex_exec_server::RouteAwareHttpClient::new(
        config.http_client_factory(),
    ));
    assert_eq!(
        dex_mcp::apply_http_headers_helper(
            login_client,
            &configured[DEX_APPS_MCP_SERVER_NAME],
            dex_home.path().to_path_buf(),
        )
        .err(),
        Some("the MCP server is disabled by managed requirements".to_string())
    );
    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(DEX_APPS_MCP_SERVER_NAME)
        .ok_or("hosted plugin runtime should remain configured")?;

    assert!(!server.enabled());
    Ok(())
}

#[tokio::test]
async fn default_fallback_overwrites_reserved_config_without_an_extension() -> TestResult {
    let dex_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .dex_home(dex_home.path().to_path_buf())
        .fallback_cwd(Some(dex_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            (
                "mcp_servers.dex_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
        ])
        .build()
        .await?;
    let auth = DexAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = McpManager::new(Arc::new(plugins_manager_for_config(
        &config,
        AuthManager::from_auth_for_testing(auth.clone()),
    )));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(DEX_APPS_MCP_SERVER_NAME)
        .ok_or("default Apps MCP should be present")?
        .config();
    let McpServerTransportConfig::StreamableHttp { url, .. } = &server.transport else {
        panic!("default Apps MCP should use streamable HTTP");
    };
    assert_eq!(url, "https://chatgpt.com/backend-api/ps/mcp");

    Ok(())
}

#[tokio::test]
async fn later_extension_can_remove_same_name_registration() -> TestResult {
    let dex_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .dex_home(dex_home.path().to_path_buf())
        .fallback_cwd(Some(dex_home.path().to_path_buf()))
        .cli_overrides(vec![("features.apps".to_string(), true.into())])
        .build()
        .await?;
    let auth = DexAuth::create_dummy_chatgpt_auth_for_testing();
    let mut builder = ExtensionRegistryBuilder::new();
    dex_mcp_extension::install(&mut builder);
    builder.mcp_server_contributor(Arc::new(RemoveDexApps));
    let manager = McpManager::new_with_extensions(
        Arc::new(plugins_manager_for_config(
            &config,
            AuthManager::from_auth_for_testing(auth.clone()),
        )),
        Arc::new(builder.build()),
        dex_core::DexAppsToolsCache::default(),
    );

    let servers = manager.effective_servers(&config, Some(&auth)).await;

    assert!(!servers.contains_key(DEX_APPS_MCP_SERVER_NAME));
    Ok(())
}

#[tokio::test]
async fn hosted_apps_mcp_requires_chatgpt_auth() -> TestResult {
    let dex_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .dex_home(dex_home.path().to_path_buf())
        .fallback_cwd(Some(dex_home.path().to_path_buf()))
        .cli_overrides(vec![("features.apps".to_string(), true.into())])
        .build()
        .await?;
    let auth = DexAuth::from_api_key("test");
    let manager = installed_manager(&config, Some(auth.clone()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    assert!(!servers.contains_key(DEX_APPS_MCP_SERVER_NAME));

    Ok(())
}

#[tokio::test]
async fn disabled_apps_remove_reserved_server_config_for_all_hosts() -> TestResult {
    let dex_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .dex_home(dex_home.path().to_path_buf())
        .fallback_cwd(Some(dex_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), false.into()),
            (
                "mcp_servers.dex_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
        ])
        .build()
        .await?;
    let managers = [
        installed_manager(&config, /*auth*/ None),
        McpManager::new(Arc::new(plugins_manager_for_config(
            &config,
            auth_manager_from_optional_auth(/*auth*/ None),
        ))),
    ];
    for manager in managers {
        let servers = manager.runtime_servers(&config).await;
        assert!(!servers.contains_key(DEX_APPS_MCP_SERVER_NAME));
    }
    Ok(())
}

fn installed_manager(config: &Config, auth: Option<DexAuth>) -> McpManager {
    let mut builder = ExtensionRegistryBuilder::new();
    dex_mcp_extension::install(&mut builder);
    McpManager::new_with_extensions(
        Arc::new(plugins_manager_for_config(
            config,
            auth_manager_from_optional_auth(auth),
        )),
        Arc::new(builder.build()),
        dex_core::DexAppsToolsCache::default(),
    )
}

struct RemoveDexApps;

impl McpServerContributor<Config> for RemoveDexApps {
    fn id(&self) -> &'static str {
        "remove_dex_apps"
    }

    fn contribute<'a>(
        &'a self,
        _context: McpServerContributionContext<'a, Config>,
    ) -> dex_extension_api::ExtensionFuture<'a, Vec<McpServerContribution>> {
        Box::pin(async move {
            vec![McpServerContribution::Remove {
                name: DEX_APPS_MCP_SERVER_NAME.to_string(),
            }]
        })
    }
}
