use super::*;
use dex_config::McpServerTransportConfig;
use dex_core::config::ConfigBuilder;
use dex_extension_api::ExtensionData;
use dex_extension_api::ExtensionDataInit;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn hosted_plugin_runtime_forwards_thread_originator() -> Result<(), Box<dyn std::error::Error>>
{
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
    let thread_init = ExtensionDataInit::new();
    let thread_store = ExtensionData::new("thread");

    let contributions = HostedPluginRuntimeExtension
        .contribute(McpServerContributionContext::for_step(
            &config,
            &thread_init,
            &thread_store,
            "dex_work_desktop",
            /*ready_selected_capability_roots*/ &[],
            /*executor_capability_discovery*/ None,
            /*executor_capability_discovery_v2*/ None,
        ))
        .await;
    let [
        McpServerContribution::HostedApps {
            config: server,
            protocol_mode: None,
        },
    ] = contributions.as_slice()
    else {
        panic!("hosted plugin runtime should contribute one server");
    };
    let McpServerTransportConfig::StreamableHttp { http_headers, .. } = &server.transport else {
        panic!("hosted plugin runtime should use streamable HTTP");
    };

    assert_eq!(
        http_headers
            .as_ref()
            .and_then(|headers| headers.get("originator")),
        Some(&"dex_work_desktop".to_string())
    );

    Ok(())
}

#[path = "event_stream_tests.rs"]
mod event_stream_tests;
#[path = "stream_manager_tests.rs"]
mod stream_manager_tests;
