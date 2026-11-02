//! Test-only helpers exposed for cross-crate integration tests.
//!
//! Production code should not depend on this module.
//! We prefer this to using a crate feature to avoid building multiple
//! permutations of the crate.

use std::path::PathBuf;
use std::sync::Arc;

use dex_exec_server::EnvironmentManager;
use dex_extension_api::LoadInstructionsFuture;
use dex_extension_api::LoadedUserInstructions;
use dex_extension_api::UserInstructionsProvider;
use dex_http_client::HttpClientFactory;
use dex_http_client::OutboundProxyPolicy;
use dex_login::AuthManager;
use dex_login::DexAuth;
use dex_model_provider::create_model_provider;
use dex_model_provider_info::ModelProviderInfo;
use dex_models_manager::bundled_models_response;
use dex_models_manager::collaboration_mode_presets;
use dex_models_manager::manager::SharedModelsManager;
use dex_models_manager::test_support::construct_model_info_offline_for_tests;
use dex_models_manager::test_support::get_model_offline_for_tests;
use dex_protocol::ThreadId;
use dex_protocol::config_types::CollaborationModeMask;
use dex_protocol::mcp::ClientMcpExtensions;
use dex_protocol::mcp::OPENAI_FORM_EXTENSION_ID;
use dex_protocol::openai_models::ModelInfo;
use dex_protocol::openai_models::ModelPreset;
use dex_protocol::protocol::SessionSource;
use once_cell::sync::Lazy;

use crate::ThreadManager;
pub use crate::code_mode_host::ProcessOwnedGrpcCodeModeSessionProvider;
use crate::config::Config;
use crate::responses_metadata::DexResponsesMetadata;
use crate::responses_metadata::DexResponsesRequestKind;
use crate::responses_metadata::subagent_header_value;
use crate::responses_metadata::subagent_metadata_kind;
use crate::thread_manager;
use crate::unified_exec;

static TEST_MODEL_PRESETS: Lazy<Vec<ModelPreset>> = Lazy::new(|| {
    let mut response = bundled_models_response()
        .unwrap_or_else(|err| panic!("bundled models.json should parse: {err}"));
    response.models.sort_by_key(|model| model.priority);
    let mut presets: Vec<ModelPreset> = response.models.into_iter().map(Into::into).collect();
    ModelPreset::mark_default_by_picker_visibility(&mut presets);
    presets
});

/// Inspect the same resolved environment configurations used to construct sampling steps.
pub async fn environment_windows_sandbox_types(
    thread: &crate::DexThread,
) -> Vec<(String, dex_sandboxing::SandboxType)> {
    thread
        .session
        .services
        .turn_environments
        .snapshot()
        .await
        .turn_environments()
        .map(|environment| {
            (
                environment.selection.environment_id.clone(),
                environment.config().windows_sandbox_type,
            )
        })
        .collect()
}

/// Reattaches request-only observations to a completed turn's history for capture assertions.
/// Tests inspect this separately from the destination-filtered HTTP/WS request.
pub async fn history_with_tool_call_metadata(
    thread: &crate::DexThread,
) -> Vec<dex_protocol::models::ResponseItem> {
    let history = thread.conversation_history_snapshot().await;
    let mut items = history.items().cloned().collect::<Vec<_>>();
    thread
        .session
        .services
        .executed_tool_calls
        .attach_to_prompt(&mut items, &mut Default::default());
    items
}

/// Returns the recorder state used by the next session metadata snapshot.
/// This does not change destination filtering or issue a request.
pub fn mcp_attribution_snapshot(thread: &crate::DexThread) -> dex_protocol::mcp::McpAttribution {
    thread
        .session
        .services
        .executed_tool_calls
        .mcp_attribution_snapshot()
}

/// Retains the active turn's timing after completion without exporting telemetry.
/// Returns `None` when there is no running task. The observer reports exclusive
/// durations for the caller's interval in inference, automatic review, local
/// tools, overhead, and unknown order.
pub async fn observe_turn_timing(
    thread: &crate::DexThread,
) -> Option<
    impl Fn(std::time::Instant, std::time::Instant) -> Option<[std::time::Duration; 5]> + use<>,
> {
    let active = thread.session.active_turn.lock().await;
    let timing = Arc::clone(
        &active
            .as_ref()?
            .task
            .as_ref()?
            .turn_context
            .turn_timing_state,
    );
    Some(move |start, end| {
        timing.snapshot_path(end).map(|path| {
            let totals = path.durations(start, end);
            [
                totals.inference,
                totals.auto_review,
                totals.tools,
                totals.overhead,
                totals.unknown,
            ]
        })
    })
}

/// Test-only provider that supplies no user instructions.
#[derive(Debug, Default)]
pub struct EmptyUserInstructionsProvider;

impl UserInstructionsProvider for EmptyUserInstructionsProvider {
    fn load_user_instructions(&self) -> LoadInstructionsFuture<'_> {
        Box::pin(async { LoadedUserInstructions::default() })
    }
}

pub fn set_thread_manager_test_mode(enabled: bool) {
    thread_manager::set_thread_manager_test_mode_for_tests(enabled);
}

pub fn set_deterministic_process_ids(enabled: bool) {
    unified_exec::set_deterministic_process_ids_for_tests(enabled);
}

pub fn auth_manager_from_auth(auth: DexAuth) -> Arc<AuthManager> {
    AuthManager::from_auth_for_testing(auth)
}

pub fn auth_manager_from_auth_with_home(auth: DexAuth, dex_home: PathBuf) -> Arc<AuthManager> {
    AuthManager::from_auth_for_testing_with_home(auth, dex_home)
}

pub fn with_code_mode_host_program(
    thread_manager: ThreadManager,
    host_program: PathBuf,
    config: &crate::config::Config,
) -> ThreadManager {
    thread_manager.with_code_mode_host_program_for_tests(host_program, config)
}

pub fn thread_manager_with_models_provider(
    auth: DexAuth,
    provider: ModelProviderInfo,
) -> ThreadManager {
    ThreadManager::with_models_provider_for_tests(auth, provider)
}

pub fn thread_manager_with_models_provider_and_home(
    auth: DexAuth,
    provider: ModelProviderInfo,
    dex_home: PathBuf,
    environment_manager: Arc<EnvironmentManager>,
) -> ThreadManager {
    ThreadManager::with_models_provider_and_home_for_tests(
        auth,
        provider,
        dex_home,
        environment_manager,
    )
}

pub async fn start_thread_with_user_shell_override(
    thread_manager: &ThreadManager,
    config: Config,
    user_shell_override: crate::shell::Shell,
    supports_openai_form_elicitation: bool,
) -> dex_protocol::error::Result<crate::NewThread> {
    thread_manager
        .start_thread_with_user_shell_override_for_tests(
            config,
            user_shell_override,
            ClientMcpExtensions::new(
                supports_openai_form_elicitation
                    .then(|| (OPENAI_FORM_EXTENSION_ID.to_string(), serde_json::json!({}))),
            ),
        )
        .await
}

pub async fn resume_legacy_thread_from_rollout_with_user_shell_override(
    thread_manager: &ThreadManager,
    config: Config,
    rollout_path: PathBuf,
    auth_manager: Arc<AuthManager>,
    user_shell_override: crate::shell::Shell,
    supports_openai_form_elicitation: bool,
) -> dex_protocol::error::Result<crate::NewThread> {
    thread_manager
        .resume_legacy_thread_from_rollout_with_user_shell_override_for_tests(
            config,
            rollout_path,
            auth_manager,
            user_shell_override,
            ClientMcpExtensions::new(
                supports_openai_form_elicitation
                    .then(|| (OPENAI_FORM_EXTENSION_ID.to_string(), serde_json::json!({}))),
            ),
        )
        .await
}

pub fn models_manager_with_provider(
    dex_home: PathBuf,
    auth_manager: Arc<AuthManager>,
    provider: ModelProviderInfo,
) -> SharedModelsManager {
    let provider = create_model_provider(provider, Some(auth_manager));
    provider.models_manager(dex_home, /*config_model_catalog*/ None)
}

pub fn default_http_client_factory() -> HttpClientFactory {
    HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
}

pub fn get_model_offline(model: Option<&str>) -> String {
    get_model_offline_for_tests(model)
}

pub fn construct_model_info_offline(model: &str, config: &Config) -> ModelInfo {
    Arc::unwrap_or_clone(
        construct_model_info_offline_for_tests(model, &config.to_models_manager_config()).model,
    )
}

#[derive(Clone, Copy)]
pub enum TestDexResponsesRequestKind {
    Turn,
    Prewarm,
    WebsocketConnection,
}

#[allow(clippy::too_many_arguments)]
pub fn responses_metadata(
    installation_id: &str,
    session_id: &str,
    thread_id: &str,
    turn_id: Option<&str>,
    window_id: String,
    session_source: &SessionSource,
    parent_thread_id: Option<ThreadId>,
    request_kind: TestDexResponsesRequestKind,
) -> DexResponsesMetadata {
    let request_kind = match request_kind {
        TestDexResponsesRequestKind::Turn => Some(DexResponsesRequestKind::Turn),
        TestDexResponsesRequestKind::Prewarm => Some(DexResponsesRequestKind::Prewarm),
        TestDexResponsesRequestKind::WebsocketConnection => None,
    };
    DexResponsesMetadata {
        turn_id: request_kind.and(turn_id.map(ToString::to_string)),
        request_kind,
        parent_thread_id,
        subagent_header: subagent_header_value(session_source),
        subagent_kind: request_kind.and_then(|_| subagent_metadata_kind(session_source)),
        ..DexResponsesMetadata::new(
            installation_id.to_string(),
            session_id.to_string(),
            thread_id.to_string(),
            window_id,
        )
    }
}

pub fn with_parent_turn(mut metadata: DexResponsesMetadata, id: &str) -> DexResponsesMetadata {
    metadata.parent_turn_id = Some(id.to_string());
    metadata
}

pub fn all_model_presets() -> &'static Vec<ModelPreset> {
    &TEST_MODEL_PRESETS
}

pub fn builtin_collaboration_mode_presets() -> Vec<CollaborationModeMask> {
    collaboration_mode_presets::builtin_collaboration_mode_presets()
}
