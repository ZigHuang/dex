use crate::events::AppServerRpcTransport;
use crate::events::GuardianReviewAnalyticsResult;
use crate::events::GuardianReviewTrackContext;
use crate::facts::AnalyticsJsonRpcError;
use crate::facts::AppInvocation;
use crate::facts::ArtifactOperation;
use crate::facts::DexGoalEvent;
use crate::facts::ExternalAgentConfigImportCompletedInput;
use crate::facts::ExternalAgentConfigImportFailureInput;
use crate::facts::HookRunFact;
use crate::facts::ImagePreparationFact;
use crate::facts::PluginInstallRequested;
use crate::facts::PluginInstallSource;
use crate::facts::PluginMeasurementsInput;
use crate::facts::SkillInvocation;
use crate::facts::SubAgentThreadStartedInput;
use crate::facts::TrackEventsContext;
use crate::facts::TurnDexErrorFact;
use crate::facts::TurnProfileFact;
use crate::facts::TurnResolvedConfigFact;
use crate::facts::TurnTokenUsageFact;
use dex_app_server_protocol::ClientRequest;
use dex_app_server_protocol::ClientResponsePayload;
use dex_app_server_protocol::InitializeParams;
use dex_app_server_protocol::JSONRPCErrorError;
use dex_app_server_protocol::RequestId;
use dex_app_server_protocol::ServerNotification;
use dex_app_server_protocol::ServerRequest;
use dex_app_server_protocol::ServerResponse;
use dex_login::AuthManager;
use dex_plugin::PluginTelemetryMetadata;
use dex_protocol::items::CollabAgentToolCallItem;
use dex_protocol::request_permissions::RequestPermissionsResponse;
use std::sync::Arc;

/// Compatibility facade for callers compiled against the former analytics API.
/// Dex does not collect, queue, or transmit product analytics.
#[derive(Clone, Default)]
pub struct AnalyticsEventsClient;

impl AnalyticsEventsClient {
    pub fn new(
        _auth_manager: Arc<AuthManager>,
        _base_url: String,
        _analytics_enabled: Option<bool>,
    ) -> Self {
        Self
    }

    pub fn disabled() -> Self {
        Self
    }

    pub async fn flush(&self) {}

    pub fn is_enabled(&self) -> bool {
        false
    }

    pub fn track_plugin_measurements(&self, _input: PluginMeasurementsInput) {}

    pub fn track_skill_invocations(
        &self,
        _tracking: TrackEventsContext,
        _invocations: Vec<SkillInvocation>,
    ) {
    }

    pub fn track_artifact_operation(
        &self,
        _tracking: TrackEventsContext,
        _operation: ArtifactOperation,
    ) {
    }

    pub fn track_initialize(
        &self,
        _connection_id: u64,
        _params: InitializeParams,
        _product_client_id: String,
        _rpc_transport: AppServerRpcTransport,
    ) {
    }

    pub fn track_subagent_thread_started(&self, _input: SubAgentThreadStartedInput) {}

    pub fn track_collab_tool_call(
        &self,
        _turn_id: String,
        _item: CollabAgentToolCallItem,
        _started_at_ms: i64,
        _completed_at_ms: i64,
    ) {
    }

    pub fn track_code_mode_tool_call(&self, _input: crate::facts::CodeModeToolCallFact) {}

    pub fn track_control_tool_call(&self, _input: crate::facts::ControlToolCallFact) {}

    pub fn track_guardian_review(
        &self,
        _tracking: &GuardianReviewTrackContext,
        _result: GuardianReviewAnalyticsResult,
        _completed_at_ms: u64,
    ) {
    }

    pub fn track_app_mentioned(
        &self,
        _tracking: TrackEventsContext,
        _mentions: Vec<AppInvocation>,
    ) {
    }

    pub fn track_request(
        &self,
        _connection_id: u64,
        _request_id: RequestId,
        _request: &ClientRequest,
    ) {
    }

    pub fn track_app_used(&self, _tracking: TrackEventsContext, _app: AppInvocation) {}

    pub fn track_hook_run(&self, _tracking: TrackEventsContext, _hook: HookRunFact) {}

    pub fn track_plugin_used(
        &self,
        _tracking: TrackEventsContext,
        _plugin: PluginTelemetryMetadata,
    ) {
    }

    pub fn track_plugin_install_requested(
        &self,
        _tracking: TrackEventsContext,
        _request: PluginInstallRequested,
    ) {
    }

    pub fn track_compaction(&self, _event: crate::facts::DexCompactionEvent) {}

    pub fn track_goal_event(&self, _event: DexGoalEvent) {}

    pub fn track_image_preparation(&self, _fact: ImagePreparationFact) {}

    pub fn track_turn_resolved_config(&self, _fact: TurnResolvedConfigFact) {}

    pub fn track_turn_token_usage(&self, _fact: TurnTokenUsageFact) {}

    pub fn track_turn_profile(&self, _fact: TurnProfileFact) {}

    pub fn track_turn_dex_error(&self, _fact: TurnDexErrorFact) {}

    pub fn track_plugin_installed(&self, _plugin: PluginTelemetryMetadata) {}

    pub fn track_plugin_install_failed(
        &self,
        _plugin: PluginTelemetryMetadata,
        _source: PluginInstallSource,
        _error_type: String,
        _sub_error_type: Option<String>,
    ) {
    }

    pub fn track_external_agent_config_import_completed(
        &self,
        _input: ExternalAgentConfigImportCompletedInput,
    ) {
    }

    pub fn track_external_agent_config_import_failure(
        &self,
        _input: ExternalAgentConfigImportFailureInput,
    ) {
    }

    pub fn track_plugin_uninstalled(&self, _plugin: PluginTelemetryMetadata) {}

    pub fn track_plugin_enabled(&self, _plugin: PluginTelemetryMetadata) {}

    pub fn track_plugin_disabled(&self, _plugin: PluginTelemetryMetadata) {}

    pub fn track_response(
        &self,
        _connection_id: u64,
        _request_id: RequestId,
        _response: &ClientResponsePayload,
    ) {
    }

    pub fn track_response_with_thread_originator(
        &self,
        _connection_id: u64,
        _request_id: RequestId,
        _response: &ClientResponsePayload,
        _thread_originator: String,
    ) {
    }

    pub fn track_error_response(
        &self,
        _connection_id: u64,
        _request_id: RequestId,
        _error: JSONRPCErrorError,
        _error_type: Option<AnalyticsJsonRpcError>,
    ) {
    }

    pub fn track_server_request(&self, _connection_id: u64, _request: ServerRequest) {}

    pub fn track_server_response(&self, _completed_at_ms: u64, _response: ServerResponse) {}

    pub fn track_effective_permissions_approval_response(
        &self,
        _completed_at_ms: u64,
        _request_id: RequestId,
        _response: RequestPermissionsResponse,
    ) {
    }

    pub fn track_server_request_aborted(&self, _completed_at_ms: u64, _request_id: RequestId) {}

    pub fn track_notification(&self, _notification: &ServerNotification) {}
}
