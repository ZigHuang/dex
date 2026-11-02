#![cfg(test)]

//! Search must retain the host's routing context across thread configuration changes.

use std::io;
use std::sync::Arc;

use dex_core::config::ConfigBuilder;
use dex_extension_api::ConfigContributor;
use dex_extension_api::ConversationHistory;
use dex_extension_api::ExtensionData;
use dex_extension_api::FunctionCallError;
use dex_extension_api::NoopTurnItemEmitter;
use dex_extension_api::ThreadLifecycleContributor;
use dex_extension_api::ThreadStartInput;
use dex_extension_api::ToolCall;
use dex_extension_api::ToolCallSource;
use dex_extension_api::ToolContributor;
use dex_extension_api::ToolName;
use dex_extension_api::ToolPayload;
use dex_login::AuthManager;
use dex_login::DexAuth;
use dex_login::WorkspaceRouting;
use dex_login::WorkspaceRoutingRequest;
use dex_login::WorkspaceRoutingResolver;
use dex_model_provider::ModelProviderFuture;
use dex_model_provider::WorkspaceRoutingContext;
use dex_model_provider_info::ModelProviderInfo;
use dex_protocol::config_types::WebSearchMode;
use dex_protocol::protocol::SessionSource;
use dex_protocol::protocol::TruncationPolicy;
use serde_json::json;

use super::WebSearchExtension;

#[tokio::test]
async fn config_changes_preserve_the_threads_established_routing() {
    struct ReportBootstrap;

    impl WorkspaceRoutingResolver for ReportBootstrap {
        fn resolve(
            &self,
            request: WorkspaceRoutingRequest,
        ) -> ModelProviderFuture<'_, io::Result<Option<WorkspaceRouting>>> {
            Box::pin(async move { Err(io::Error::other(request.chatgpt_base_url)) })
        }
    }

    let home = tempfile::tempdir().unwrap();
    let mut config = ConfigBuilder::default()
        .dex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .build()
        .await
        .unwrap();
    config.web_search_mode.set(WebSearchMode::Cached).unwrap();
    config.model_provider = ModelProviderInfo::create_openai_provider(Some(
        "http://127.0.0.1:0/backend-api/dex".into(),
    ));
    let auth_manager =
        AuthManager::from_auth_for_testing(DexAuth::create_dummy_chatgpt_auth_for_testing());
    let resolver: Arc<dyn WorkspaceRoutingResolver> = Arc::new(ReportBootstrap);
    auth_manager.set_workspace_routing_resolver(Arc::downgrade(&resolver));
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    // Use a thread bootstrap distinct from either config so reconstructing it is observable.
    let thread_bootstrap = "https://thread.example/backend-api";
    thread_store.insert(WorkspaceRoutingContext::new(thread_bootstrap.into()));
    let extension = WebSearchExtension { auth_manager };
    extension
        .on_thread_start(ThreadStartInput {
            config: &config,
            session_source: &SessionSource::Exec,
            persistent_thread_state_available: false,
            environments: &[],
            mcp_resource_client: None,
            extension_metrics: None,
            session_store: &session_store,
            thread_store: &thread_store,
        })
        .await;
    let mut changed = config.clone();
    changed.chatgpt_base_url = "https://changed.example/backend-api".into();
    extension.on_config_changed(&session_store, &thread_store, &config, &changed);
    let step_store = ExtensionData::new("step");
    step_store.insert(config.web_search_mode.value());
    let tools = extension.tools_for_step(&session_store, &thread_store, &step_store);
    let call = ToolCall {
        turn_id: "turn".into(),
        call_id: "search".into(),
        tool_name: ToolName::namespaced("web", "run"),
        model: "test-model".into(),
        dex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(1024),
        source: ToolCallSource::Direct,
        conversation_history: ConversationHistory::default(),
        turn_item_emitter: Arc::new(NoopTurnItemEmitter),
        attachment_store: dex_core::passthrough_image_store(),
        ephemeral: false,
        environments: Vec::new(),
        payload: ToolPayload::Function {
            arguments: json!({"search_query": [{"q": "test"}]}).to_string(),
        },
    };
    assert!(matches!(
        tools[0].handle(call).await,
        Err(FunctionCallError::RespondToModel(message)) if message == thread_bootstrap
    ));
}
