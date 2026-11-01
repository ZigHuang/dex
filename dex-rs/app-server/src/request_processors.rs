use crate::bespoke_event_handling::apply_bespoke_event_handling;
use crate::command_exec::CommandExecManager;
use crate::command_exec::StartCommandExecParams;
use crate::config_manager::ConfigManager;
use crate::error_code::INPUT_TOO_LARGE_ERROR_CODE;
use crate::error_code::invalid_params;
use crate::models::supported_models;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::RequestContext;
use crate::outgoing_message::ThreadScopedOutgoingMessageSender;
use crate::skills_watcher::SkillsWatcher;
use crate::thread_status::ThreadWatchManager;
use crate::thread_status::resolve_thread_status;
use chrono::Duration as ChronoDuration;
use chrono::SecondsFormat;
use dex_analytics::AnalyticsEventsClient;
use dex_analytics::AnalyticsJsonRpcError;
use dex_analytics::InputError;
use dex_analytics::TurnSteerRequestError;
use dex_app_server_protocol::Account;
use dex_app_server_protocol::AccountLoginCompletedNotification;
use dex_app_server_protocol::AccountTokenUsageDailyBucket;
use dex_app_server_protocol::AccountTokenUsageSummary;
use dex_app_server_protocol::AccountUpdatedNotification;
use dex_app_server_protocol::AddCreditsNudgeCreditType;
use dex_app_server_protocol::AddCreditsNudgeEmailStatus;
use dex_app_server_protocol::AdditionalContextEntry;
use dex_app_server_protocol::AdditionalContextKind;
use dex_app_server_protocol::AppListUpdatedNotification;
use dex_app_server_protocol::AppSummary;
use dex_app_server_protocol::AppTemplateSummary;
use dex_app_server_protocol::AppTemplateUnavailableReason;
use dex_app_server_protocol::AppsInstalledParams;
use dex_app_server_protocol::AppsInstalledResponse;
use dex_app_server_protocol::AppsListParams;
use dex_app_server_protocol::AppsListResponse;
use dex_app_server_protocol::AppsReadParams;
use dex_app_server_protocol::AppsReadResponse;
use dex_app_server_protocol::AskForApproval;
use dex_app_server_protocol::AuthMode;
use dex_app_server_protocol::CancelLoginAccountParams;
use dex_app_server_protocol::CancelLoginAccountResponse;
use dex_app_server_protocol::CancelLoginAccountStatus;
use dex_app_server_protocol::ClientInfo;
use dex_app_server_protocol::ClientRequest;
use dex_app_server_protocol::ClientResponsePayload;
use dex_app_server_protocol::CollaborationModeListParams;
use dex_app_server_protocol::CollaborationModeListResponse;
use dex_app_server_protocol::CommandExecParams;
use dex_app_server_protocol::CommandExecResizeParams;
use dex_app_server_protocol::CommandExecTerminateParams;
use dex_app_server_protocol::CommandExecWriteParams;
use dex_app_server_protocol::ConfigWarningNotification;
use dex_app_server_protocol::ConsumeAccountRateLimitResetCreditOutcome;
use dex_app_server_protocol::ConsumeAccountRateLimitResetCreditParams;
use dex_app_server_protocol::ConsumeAccountRateLimitResetCreditResponse;
use dex_app_server_protocol::ConversationGitInfo;
use dex_app_server_protocol::ConversationSummary;
use dex_app_server_protocol::DeprecationNoticeNotification;
use dex_app_server_protocol::DexErrorInfo;
use dex_app_server_protocol::DynamicToolFunctionSpec;
use dex_app_server_protocol::DynamicToolNamespaceTool;
use dex_app_server_protocol::DynamicToolSpec;
use dex_app_server_protocol::EnvironmentAddParams;
use dex_app_server_protocol::EnvironmentAddResponse;
use dex_app_server_protocol::EnvironmentInfoParams;
use dex_app_server_protocol::EnvironmentInfoResponse;
use dex_app_server_protocol::EnvironmentShellInfo;
use dex_app_server_protocol::EnvironmentStatusKind;
use dex_app_server_protocol::EnvironmentStatusParams;
use dex_app_server_protocol::EnvironmentStatusResponse;
use dex_app_server_protocol::ExperimentalFeature as ApiExperimentalFeature;
use dex_app_server_protocol::ExperimentalFeatureListParams;
use dex_app_server_protocol::ExperimentalFeatureListResponse;
use dex_app_server_protocol::ExperimentalFeatureStage as ApiExperimentalFeatureStage;
use dex_app_server_protocol::FeedbackUploadParams;
use dex_app_server_protocol::GetAccountParams;
use dex_app_server_protocol::GetAccountRateLimitsResponse;
use dex_app_server_protocol::GetAccountResponse;
use dex_app_server_protocol::GetAccountTokenUsageParams;
use dex_app_server_protocol::GetAccountTokenUsageResponse;
use dex_app_server_protocol::GetAuthStatusParams;
use dex_app_server_protocol::GetAuthStatusResponse;
use dex_app_server_protocol::GetConversationSummaryParams;
use dex_app_server_protocol::GetConversationSummaryResponse;
use dex_app_server_protocol::GetWorkspaceMessagesResponse;
use dex_app_server_protocol::GitDiffToRemoteParams;
use dex_app_server_protocol::GitDiffToRemoteResponse;
use dex_app_server_protocol::GitInfo as ApiGitInfo;
use dex_app_server_protocol::HookHandlerMetadata;
use dex_app_server_protocol::HookMetadata;
use dex_app_server_protocol::HooksListParams;
use dex_app_server_protocol::HooksListResponse;
use dex_app_server_protocol::InitializeParams;
use dex_app_server_protocol::InitializeResponse;
use dex_app_server_protocol::InstalledApp;
use dex_app_server_protocol::JSONRPCErrorError;
use dex_app_server_protocol::ListMcpServerStatusParams;
use dex_app_server_protocol::ListMcpServerStatusResponse;
use dex_app_server_protocol::LoginAccountParams;
use dex_app_server_protocol::LoginAccountResponse;
use dex_app_server_protocol::LoginApiKeyParams;
use dex_app_server_protocol::LoginAppBrand;
use dex_app_server_protocol::LogoutAccountResponse;
use dex_app_server_protocol::MarketplaceAddParams;
use dex_app_server_protocol::MarketplaceAddResponse;
use dex_app_server_protocol::MarketplaceInterface;
use dex_app_server_protocol::MarketplaceRemoveParams;
use dex_app_server_protocol::MarketplaceRemoveResponse;
use dex_app_server_protocol::MarketplaceUpgradeErrorInfo;
use dex_app_server_protocol::MarketplaceUpgradeParams;
use dex_app_server_protocol::MarketplaceUpgradeResponse;
use dex_app_server_protocol::McpResourceReadParams;
use dex_app_server_protocol::McpResourceReadResponse;
use dex_app_server_protocol::McpServerOauthClientRegistration;
use dex_app_server_protocol::McpServerOauthLoginCompletedNotification;
use dex_app_server_protocol::McpServerOauthLoginParams;
use dex_app_server_protocol::McpServerOauthLoginResponse;
use dex_app_server_protocol::McpServerRefreshResponse;
use dex_app_server_protocol::McpServerStatus;
use dex_app_server_protocol::McpServerStatusDetail;
use dex_app_server_protocol::McpServerToolCallParams;
use dex_app_server_protocol::McpServerToolCallResponse;
use dex_app_server_protocol::MemoryResetResponse;
use dex_app_server_protocol::MockExperimentalMethodParams;
use dex_app_server_protocol::MockExperimentalMethodResponse;
use dex_app_server_protocol::ModelListParams;
use dex_app_server_protocol::ModelListResponse;
use dex_app_server_protocol::PermissionProfileListParams;
use dex_app_server_protocol::PermissionProfileListResponse;
use dex_app_server_protocol::PermissionProfileSummary;
use dex_app_server_protocol::PluginDetail;
use dex_app_server_protocol::PluginInstallParams;
use dex_app_server_protocol::PluginInstallResponse;
use dex_app_server_protocol::PluginInstalledParams;
use dex_app_server_protocol::PluginInstalledResponse;
use dex_app_server_protocol::PluginInterface;
use dex_app_server_protocol::PluginListMarketplaceKind;
use dex_app_server_protocol::PluginListParams;
use dex_app_server_protocol::PluginListResponse;
use dex_app_server_protocol::PluginMarketplaceEntry;
use dex_app_server_protocol::PluginReadParams;
use dex_app_server_protocol::PluginReadResponse;
use dex_app_server_protocol::PluginShareCheckoutParams;
use dex_app_server_protocol::PluginShareCheckoutResponse;
use dex_app_server_protocol::PluginShareContext;
use dex_app_server_protocol::PluginShareDeleteParams;
use dex_app_server_protocol::PluginShareDeleteResponse;
use dex_app_server_protocol::PluginShareDiscoverability;
use dex_app_server_protocol::PluginShareListItem;
use dex_app_server_protocol::PluginShareListParams;
use dex_app_server_protocol::PluginShareListResponse;
use dex_app_server_protocol::PluginSharePrincipal;
use dex_app_server_protocol::PluginSharePrincipalType;
use dex_app_server_protocol::PluginShareSaveParams;
use dex_app_server_protocol::PluginShareSaveResponse;
use dex_app_server_protocol::PluginShareTarget;
use dex_app_server_protocol::PluginShareUpdateDiscoverability;
use dex_app_server_protocol::PluginShareUpdateTargetsParams;
use dex_app_server_protocol::PluginShareUpdateTargetsResponse;
use dex_app_server_protocol::PluginSkillReadParams;
use dex_app_server_protocol::PluginSkillReadResponse;
use dex_app_server_protocol::PluginSource;
use dex_app_server_protocol::PluginSummary;
use dex_app_server_protocol::PluginUninstallParams;
use dex_app_server_protocol::PluginUninstallResponse;
use dex_app_server_protocol::RateLimitResetCredit;
use dex_app_server_protocol::RateLimitResetCreditStatus;
use dex_app_server_protocol::RateLimitResetCreditsSummary;
use dex_app_server_protocol::RateLimitResetType;
use dex_app_server_protocol::RequestId;
use dex_app_server_protocol::ReviewDelivery as ApiReviewDelivery;
use dex_app_server_protocol::ReviewStartParams;
use dex_app_server_protocol::ReviewStartResponse;
use dex_app_server_protocol::ReviewTarget as ApiReviewTarget;
use dex_app_server_protocol::SandboxMode;
use dex_app_server_protocol::SendAddCreditsNudgeEmailParams;
use dex_app_server_protocol::SendAddCreditsNudgeEmailResponse;
use dex_app_server_protocol::ServerNotification;
use dex_app_server_protocol::ServerRequestResolvedNotification;
use dex_app_server_protocol::SkillSummary;
use dex_app_server_protocol::SkillsConfigWriteParams;
use dex_app_server_protocol::SkillsConfigWriteResponse;
use dex_app_server_protocol::SkillsExtraRootsSetParams;
use dex_app_server_protocol::SkillsExtraRootsSetResponse;
use dex_app_server_protocol::SkillsListParams;
use dex_app_server_protocol::SkillsListResponse;
use dex_app_server_protocol::SortDirection;
use dex_app_server_protocol::Thread;
use dex_app_server_protocol::ThreadApproveGuardianDeniedActionParams;
use dex_app_server_protocol::ThreadApproveGuardianDeniedActionResponse;
use dex_app_server_protocol::ThreadArchiveParams;
use dex_app_server_protocol::ThreadArchiveResponse;
use dex_app_server_protocol::ThreadArchivedNotification;
use dex_app_server_protocol::ThreadBackgroundTerminal;
use dex_app_server_protocol::ThreadBackgroundTerminalsCleanParams;
use dex_app_server_protocol::ThreadBackgroundTerminalsCleanResponse;
use dex_app_server_protocol::ThreadBackgroundTerminalsListParams;
use dex_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use dex_app_server_protocol::ThreadBackgroundTerminalsTerminateParams;
use dex_app_server_protocol::ThreadBackgroundTerminalsTerminateResponse;
use dex_app_server_protocol::ThreadClosedNotification;
use dex_app_server_protocol::ThreadCompactStartParams;
use dex_app_server_protocol::ThreadCompactStartResponse;
use dex_app_server_protocol::ThreadDecrementElicitationParams;
use dex_app_server_protocol::ThreadDecrementElicitationResponse;
use dex_app_server_protocol::ThreadDeleteParams;
use dex_app_server_protocol::ThreadDeleteResponse;
use dex_app_server_protocol::ThreadDeletedNotification;
use dex_app_server_protocol::ThreadForkParams;
use dex_app_server_protocol::ThreadForkResponse;
use dex_app_server_protocol::ThreadGoal;
use dex_app_server_protocol::ThreadGoalClearParams;
use dex_app_server_protocol::ThreadGoalClearResponse;
use dex_app_server_protocol::ThreadGoalClearedNotification;
use dex_app_server_protocol::ThreadGoalGetParams;
use dex_app_server_protocol::ThreadGoalGetResponse;
use dex_app_server_protocol::ThreadGoalSetParams;
use dex_app_server_protocol::ThreadGoalSetResponse;
use dex_app_server_protocol::ThreadGoalStatus;
use dex_app_server_protocol::ThreadGoalUpdatedNotification;
use dex_app_server_protocol::ThreadHistoryBuilder;
#[cfg(test)]
use dex_app_server_protocol::ThreadHistoryMode;
use dex_app_server_protocol::ThreadIncrementElicitationParams;
use dex_app_server_protocol::ThreadIncrementElicitationResponse;
use dex_app_server_protocol::ThreadInjectItemsParams;
use dex_app_server_protocol::ThreadInjectItemsResponse;
use dex_app_server_protocol::ThreadItem;
use dex_app_server_protocol::ThreadItemEntry;
use dex_app_server_protocol::ThreadItemsListParams;
use dex_app_server_protocol::ThreadItemsListResponse;
use dex_app_server_protocol::ThreadListCwdFilter;
use dex_app_server_protocol::ThreadListParams;
use dex_app_server_protocol::ThreadListResponse;
use dex_app_server_protocol::ThreadLoadedListParams;
use dex_app_server_protocol::ThreadLoadedListResponse;
use dex_app_server_protocol::ThreadMemoryModeSetParams;
use dex_app_server_protocol::ThreadMemoryModeSetResponse;
use dex_app_server_protocol::ThreadMetadataGitInfoUpdateParams;
use dex_app_server_protocol::ThreadMetadataUpdateParams;
use dex_app_server_protocol::ThreadMetadataUpdateResponse;
use dex_app_server_protocol::ThreadNameUpdatedNotification;
use dex_app_server_protocol::ThreadProjectUpdatedNotification;
use dex_app_server_protocol::ThreadReadParams;
use dex_app_server_protocol::ThreadReadResponse;
use dex_app_server_protocol::ThreadRealtimeAppendAudioParams;
use dex_app_server_protocol::ThreadRealtimeAppendAudioResponse;
use dex_app_server_protocol::ThreadRealtimeAppendSpeechParams;
use dex_app_server_protocol::ThreadRealtimeAppendSpeechResponse;
use dex_app_server_protocol::ThreadRealtimeAppendTextParams;
use dex_app_server_protocol::ThreadRealtimeAppendTextResponse;
use dex_app_server_protocol::ThreadRealtimeListVoicesResponse;
use dex_app_server_protocol::ThreadRealtimeStartParams;
use dex_app_server_protocol::ThreadRealtimeStartResponse;
use dex_app_server_protocol::ThreadRealtimeStartTransport;
use dex_app_server_protocol::ThreadRealtimeStopParams;
use dex_app_server_protocol::ThreadRealtimeStopResponse;
use dex_app_server_protocol::ThreadResumeInitialTurnsPageParams;
use dex_app_server_protocol::ThreadResumeParams;
use dex_app_server_protocol::ThreadResumeResponse;
use dex_app_server_protocol::ThreadRollbackParams;
use dex_app_server_protocol::ThreadSearchOccurrence;
use dex_app_server_protocol::ThreadSearchOccurrencesParams;
use dex_app_server_protocol::ThreadSearchOccurrencesResponse;
use dex_app_server_protocol::ThreadSearchParams;
use dex_app_server_protocol::ThreadSearchResponse;
use dex_app_server_protocol::ThreadSearchResult;
use dex_app_server_protocol::ThreadSearchSortKey;
use dex_app_server_protocol::ThreadSearchTextRange;
use dex_app_server_protocol::ThreadSetNameParams;
use dex_app_server_protocol::ThreadSetNameResponse;
use dex_app_server_protocol::ThreadSettings;
use dex_app_server_protocol::ThreadSettingsUpdateParams;
use dex_app_server_protocol::ThreadSettingsUpdateResponse;
use dex_app_server_protocol::ThreadShellCommandParams;
use dex_app_server_protocol::ThreadShellCommandResponse;
use dex_app_server_protocol::ThreadSortKey;
use dex_app_server_protocol::ThreadSourceKind;
use dex_app_server_protocol::ThreadStartParams;
use dex_app_server_protocol::ThreadStartResponse;
use dex_app_server_protocol::ThreadStartedNotification;
use dex_app_server_protocol::ThreadStatus;
use dex_app_server_protocol::ThreadTurnsListParams;
use dex_app_server_protocol::ThreadTurnsListResponse;
use dex_app_server_protocol::ThreadUnarchiveParams;
use dex_app_server_protocol::ThreadUnarchiveResponse;
use dex_app_server_protocol::ThreadUnarchivedNotification;
use dex_app_server_protocol::ThreadUnsubscribeParams;
use dex_app_server_protocol::ThreadUnsubscribeResponse;
use dex_app_server_protocol::ThreadUnsubscribeStatus;
use dex_app_server_protocol::Turn;
use dex_app_server_protocol::TurnEnvironmentParams;
use dex_app_server_protocol::TurnError;
use dex_app_server_protocol::TurnInterruptParams;
use dex_app_server_protocol::TurnInterruptResponse;
use dex_app_server_protocol::TurnItemsView;
use dex_app_server_protocol::TurnStartParams;
use dex_app_server_protocol::TurnStartResponse;
use dex_app_server_protocol::TurnStatus;
use dex_app_server_protocol::TurnSteerParams;
use dex_app_server_protocol::TurnSteerResponse;
use dex_app_server_protocol::UserInput as V2UserInput;
use dex_app_server_protocol::WindowsSandboxReadiness;
use dex_app_server_protocol::WindowsSandboxReadinessResponse;
use dex_app_server_protocol::WindowsSandboxSetupCompletedNotification;
use dex_app_server_protocol::WindowsSandboxSetupMode;
use dex_app_server_protocol::WindowsSandboxSetupStartParams;
use dex_app_server_protocol::WindowsSandboxSetupStartResponse;
use dex_app_server_protocol::WorkspaceMessage;
use dex_app_server_protocol::WorkspaceMessageType;
use dex_arg0::Arg0DispatchPaths;
use dex_backend_client::AddCreditsNudgeCreditType as BackendAddCreditsNudgeCreditType;
use dex_backend_client::Client as BackendClient;
use dex_backend_client::ConsumeRateLimitResetCreditCode as BackendConsumeRateLimitResetCreditCode;
use dex_backend_client::DexWorkspaceMessage as BackendWorkspaceMessage;
use dex_backend_client::DexWorkspaceMessageType as BackendWorkspaceMessageType;
use dex_backend_client::DexWorkspaceMessagesResponse as BackendWorkspaceMessagesResponse;
use dex_backend_client::RateLimitResetCreditDetails as BackendRateLimitResetCreditDetails;
use dex_backend_client::RateLimitResetCreditsDetails as BackendRateLimitResetCreditsDetails;
use dex_backend_client::RequestError as BackendRequestError;
use dex_backend_client::TokenUsageProfile;
use dex_chatgpt::connectors;
use dex_config::CloudConfigBundleLoadError;
use dex_config::CloudConfigBundleLoadErrorCode;
use dex_config::ConfigLayerStack;
use dex_config::loader::project_trust_key;
use dex_config::types::McpServerTransportConfig;
use dex_connectors::AppInfo;
use dex_core::DexThread;
use dex_core::DexThreadSettingsOverrides;
use dex_core::ForkSnapshot;
use dex_core::McpManager;
use dex_core::NewThread;
use dex_core::NotSubmittedReason;
#[cfg(test)]
use dex_core::SessionMeta;
use dex_core::StartThreadOptions;
use dex_core::SteerSubmission;
use dex_core::ThreadConfigSnapshot;
use dex_core::ThreadManager;
use dex_core::TurnInput;
use dex_core::TurnInputRequest;
use dex_core::TurnInputSubmission;
use dex_core::TurnStartOptions;
use dex_core::config::Config;
use dex_core::config::ConfigOverrides;
use dex_core::config::NetworkProxyAuditMetadata;
use dex_core::config::edit::ConfigEdit;
use dex_core::config::edit::ConfigEditsBuilder;
use dex_core::connectors::AccessibleConnectorsStatus;
use dex_core::exec::ExecCapturePolicy;
use dex_core::exec::ExecExpiration;
use dex_core::exec::ExecParams;
use dex_core::exec_env::create_env;
use dex_core::path_utils;
#[cfg(test)]
use dex_core::read_head_for_summary;
use dex_core::sandboxing::SandboxPermissions;
use dex_core::truncate_rollout_after_turn_id;
use dex_core::truncate_rollout_before_turn_id;
use dex_core::windows_sandbox::WindowsSandboxLevelExt;
use dex_core::windows_sandbox::WindowsSandboxSetupMode as CoreWindowsSandboxSetupMode;
use dex_core::windows_sandbox::WindowsSandboxSetupRequest;
use dex_core::windows_sandbox::sandbox_setup_is_complete;
use dex_core_plugins::PluginInstallError as CorePluginInstallError;
use dex_core_plugins::PluginInstallRequest;
use dex_core_plugins::PluginReadRequest;
use dex_core_plugins::PluginUninstallError as CorePluginUninstallError;
use dex_core_plugins::PluginsManager;
use dex_core_plugins::loader::load_plugin_apps;
use dex_core_plugins::manifest::PluginManifestInterface;
use dex_core_plugins::marketplace::MarketplaceError;
use dex_core_plugins::marketplace::MarketplacePluginSource;
use dex_core_plugins::marketplace_add::MarketplaceAddError;
use dex_core_plugins::marketplace_add::MarketplaceAddRequest;
use dex_core_plugins::marketplace_add::add_marketplace as add_marketplace_to_dex_home;
use dex_core_plugins::marketplace_remove::MarketplaceRemoveError;
use dex_core_plugins::marketplace_remove::MarketplaceRemoveRequest as CoreMarketplaceRemoveRequest;
use dex_core_plugins::marketplace_remove::remove_marketplace;
use dex_core_plugins::remote::RemoteMarketplace;
use dex_core_plugins::remote::RemoteMarketplaceSource;
use dex_core_plugins::remote::RemotePluginCatalogError;
use dex_core_plugins::remote::RemotePluginDetail as RemoteCatalogPluginDetail;
use dex_core_plugins::remote::RemotePluginServiceConfig;
use dex_core_plugins::remote::RemotePluginShareContext as RemoteCatalogPluginShareContext;
use dex_core_plugins::remote::RemotePluginShareSummary as RemoteCatalogPluginShareSummary;
use dex_core_plugins::remote::RemotePluginSummary as RemoteCatalogPluginSummary;
use dex_exec_server::EnvironmentManager;
use dex_exec_server::EnvironmentObservedStatus;
use dex_exec_server::LOCAL_ENVIRONMENT_ID;
use dex_exec_server::LOCAL_FS;
use dex_features::FEATURES;
use dex_features::Feature;
use dex_features::Stage;
use dex_feedback::DexFeedback;
use dex_git_utils::git_diff_to_remote;
use dex_git_utils::resolve_root_git_project_for_trust;
use dex_login::AuthManager;
use dex_login::DEX_OPEN_APP_URL;
use dex_login::DexAuth;
use dex_login::LoginSuccessPage;
use dex_login::LoginSuccessPageBrand;
use dex_login::ServerOptions as LoginServerOptions;
use dex_login::ShutdownHandle;
use dex_login::complete_device_code_login;
use dex_login::login_with_api_key;
use dex_login::login_with_bedrock_api_key;
use dex_login::oauth_client_id;
use dex_login::request_device_code;
use dex_login::run_login_server;
use dex_mcp::McpRuntimeContext;
use dex_mcp::McpServerStatusSnapshot;
use dex_mcp::McpSnapshotDetail;
use dex_mcp::collect_mcp_server_status_snapshot_with_detail;
use dex_mcp::discover_supported_scopes;
use dex_mcp::read_mcp_resource as read_mcp_resource_without_thread;
use dex_mcp::resolve_oauth_scopes;
use dex_memories_write::clear_memory_roots_contents;
use dex_model_provider::create_model_provider;
use dex_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets;
use dex_protocol::ThreadId;
use dex_protocol::config_types::CollaborationMode;
use dex_protocol::config_types::ForcedLoginMethod;
use dex_protocol::config_types::Personality;
use dex_protocol::config_types::ReasoningSummary;
use dex_protocol::config_types::TrustLevel;
use dex_protocol::config_types::WindowsSandboxLevel;
use dex_protocol::error::DexErr;
use dex_protocol::error::Result as DexResult;
#[cfg(test)]
use dex_protocol::items::TurnItem;
use dex_protocol::models::ResponseItem;
use dex_protocol::openai_models::ReasoningEffort;
use dex_protocol::protocol::AgentStatus;
use dex_protocol::protocol::ConversationAudioParams;
use dex_protocol::protocol::ConversationSpeechParams;
use dex_protocol::protocol::ConversationStartParams;
use dex_protocol::protocol::ConversationStartTransport;
use dex_protocol::protocol::ConversationTextParams;
use dex_protocol::protocol::EnvironmentConfigState;
use dex_protocol::protocol::EventMsg;
#[cfg(test)]
use dex_protocol::protocol::GitInfo as CoreGitInfo;
use dex_protocol::protocol::McpAuthStatus as CoreMcpAuthStatus;
use dex_protocol::protocol::Op;
use dex_protocol::protocol::RealtimeVoicesList;
use dex_protocol::protocol::ReviewDelivery as CoreReviewDelivery;
use dex_protocol::protocol::ReviewRequest;
use dex_protocol::protocol::ReviewTarget as CoreReviewTarget;
use dex_protocol::protocol::SessionConfiguredEvent;
#[cfg(test)]
use dex_protocol::protocol::SessionMetaLine;
use dex_protocol::protocol::TurnEnvironmentSelection;
use dex_protocol::protocol::TurnEnvironmentSelections;
use dex_protocol::protocol::W3cTraceContext;
use dex_protocol::protocol::strip_user_message_prefix;
use dex_protocol::user_input::MAX_USER_INPUT_TEXT_CHARS;
use dex_protocol::user_input::UserInput as CoreInputItem;
use dex_rmcp_client::McpOAuthClientRegistration;
use dex_rmcp_client::StreamableHttpRedirectMode;
use dex_rmcp_client::perform_oauth_login_return_url;
use dex_rollout::InitialHistory;
use dex_rollout::ResumedHistory;
use dex_rollout::RolloutItem;
use dex_rollout::is_persisted_rollout_item;
use dex_rollout::state_db::StateDbHandle;
use dex_rollout::state_db::reconcile_rollout;
use dex_state::ThreadMetadata;
use dex_state::log_db::LogDbLayer;
use dex_thread_store::ArchiveThreadParams as StoreArchiveThreadParams;
use dex_thread_store::ArchiveThreadsParams as StoreArchiveThreadsParams;
use dex_thread_store::ClearableField as StoreClearableField;
use dex_thread_store::DeleteThreadsParams as StoreDeleteThreadsParams;
use dex_thread_store::GitInfoPatch as StoreGitInfoPatch;
use dex_thread_store::ItemSortKey as StoreItemSortKey;
use dex_thread_store::ListItemsParams as StoreListItemsParams;
use dex_thread_store::ListThreadsParams as StoreListThreadsParams;
use dex_thread_store::ListTurnsParams as StoreListTurnsParams;
use dex_thread_store::LoadThreadHistoryParams as StoreLoadThreadHistoryParams;
use dex_thread_store::LocalThreadStore;
use dex_thread_store::ReadThreadByRolloutPathParams as StoreReadThreadByRolloutPathParams;
use dex_thread_store::ReadThreadParams as StoreReadThreadParams;
use dex_thread_store::SearchThreadOccurrencesParams as StoreSearchThreadOccurrencesParams;
use dex_thread_store::SearchThreadsParams as StoreSearchThreadsParams;
use dex_thread_store::SortDirection as StoreSortDirection;
use dex_thread_store::StoredThread;
use dex_thread_store::StoredTurn;
use dex_thread_store::StoredTurnItemsView;
use dex_thread_store::StoredTurnStatus;
use dex_thread_store::ThreadMetadataPatch as StoreThreadMetadataPatch;
use dex_thread_store::ThreadRelationFilter as StoreThreadRelationFilter;
use dex_thread_store::ThreadSortKey as StoreThreadSortKey;
use dex_thread_store::ThreadStore;
use dex_thread_store::ThreadStoreError;
use dex_utils_absolute_path::AbsolutePathBuf;
use dex_utils_pty::DEFAULT_OUTPUT_BYTES_CAP;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;
use tokio_util::task::TaskTracker;
use toml::Value as TomlValue;
use tracing::Instrument;
use tracing::error;
use tracing::info;
use tracing::warn;
use uuid::Uuid;

#[cfg(test)]
use dex_app_server_protocol::ServerRequest;

mod account_processor;
mod apps_processor;
mod bedrock_auth;
mod catalog_processor;
mod command_exec_processor;
mod config_processor;
mod diagnostics;
mod environment_processor;
mod feedback_processor;
mod fs_processor;
mod git_processor;
mod initialize_processor;
mod marketplace_processor;
mod mcp_event_stream;
mod mcp_processor;
mod persisted_resume_settings;
mod plugins;
mod process_exec_processor;
mod projects;
mod remote_control_processor;
mod search;
mod thread_enrichment;
mod thread_fork_goal;
mod thread_processor;
mod thread_queue_processor;
mod thread_sections;
mod token_usage_replay;
mod turn_processor;
mod windows_sandbox_processor;

pub(crate) use account_processor::AccountRequestProcessor;
pub(crate) use apps_processor::AppsRequestProcessor;
pub(crate) use catalog_processor::CatalogRequestProcessor;
pub(crate) use command_exec_processor::CommandExecRequestProcessor;
pub(crate) use config_processor::ConfigRequestProcessor;
pub(crate) use diagnostics::read_server_diagnostics;
pub(crate) use environment_processor::EnvironmentRequestProcessor;
pub(crate) use feedback_processor::FeedbackRequestProcessor;
pub(crate) use fs_processor::FsRequestProcessor;
pub(crate) use git_processor::GitRequestProcessor;
pub(crate) use initialize_processor::InitializeRequestProcessor;
pub(crate) use marketplace_processor::MarketplaceRequestProcessor;
pub(crate) use mcp_event_stream::McpEventStreamReady;
pub(crate) use mcp_event_stream::McpEventStreams;
pub(crate) use mcp_processor::McpRequestProcessor;
pub(crate) use plugins::PluginRequestProcessor;
pub(crate) use process_exec_processor::ProcessExecRequestProcessor;
pub(crate) use projects::ProjectRequestProcessor;
pub(crate) use remote_control_processor::RemoteControlRequestProcessor;
pub(crate) use search::SearchRequestProcessor;
pub(crate) use thread_goal_processor::ThreadGoalRequestProcessor;
pub(crate) use thread_processor::ThreadRequestProcessor;
pub(crate) use thread_queue_processor::ThreadQueueRequestProcessor;
pub(crate) use turn_processor::TurnRequestProcessor;
pub(crate) use windows_sandbox_processor::WindowsSandboxRequestProcessor;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::filters::compute_source_filters;
use crate::filters::source_kind_matches;
use crate::thread_state::ConnectionCapabilities;
use crate::thread_state::ThreadListenerCommand;
use crate::thread_state::ThreadState;
use crate::thread_state::ThreadStateManager;
use token_usage_replay::restored_token_usage_turn_id;
use token_usage_replay::send_thread_token_usage_update_to_connection;

fn resolve_request_cwd(cwd: Option<PathBuf>) -> Result<Option<AbsolutePathBuf>, JSONRPCErrorError> {
    cwd.map(|cwd| {
        AbsolutePathBuf::relative_to_current_dir(path_utils::normalize_for_native_workdir(cwd))
            .map_err(|err| invalid_request(format!("invalid cwd: {err}")))
    })
    .transpose()
}

fn resolve_turn_environment_selections(
    thread_manager: &ThreadManager,
    environments: Option<Vec<TurnEnvironmentParams>>,
) -> Result<Option<Vec<TurnEnvironmentSelection>>, JSONRPCErrorError> {
    let Some(environments) = environments else {
        return Ok(None);
    };
    let mut selections = Vec::with_capacity(environments.len());
    for environment in environments {
        let environment_id = environment.environment_id;
        let cwd = environment
            .cwd
            .to_inferred_path_uri()
            .ok_or_else(|| {
                invalid_request(format!(
                    "invalid cwd for environment `{environment_id}`: path `{}` does not use absolute POSIX or Windows path syntax",
                    environment.cwd
                ))
            })?;
        let workspace_roots = environment
            .runtime_workspace_roots
            .map(|roots| {
                let mut resolved_roots = Vec::new();
                for root in roots {
                    let root = root.to_inferred_path_uri().ok_or_else(|| {
                        invalid_request(format!(
                            "invalid runtime workspace root for environment `{environment_id}`: path `{root}` does not use absolute POSIX or Windows path syntax"
                        ))
                    })?;
                    if !resolved_roots.contains(&root) {
                        resolved_roots.push(root);
                    }
                }
                Ok::<_, JSONRPCErrorError>(resolved_roots)
            })
            .transpose()?
            .unwrap_or_else(|| vec![cwd.clone()]);
        selections.push(TurnEnvironmentSelection {
            environment_id,
            cwd,
            workspace_roots,
            config: EnvironmentConfigState::FromThread,
        });
    }
    thread_manager
        .validate_environment_selections(&selections)
        .map_err(environment_selection_error)?;
    Ok(Some(selections))
}

fn resolve_runtime_workspace_roots(workspace_roots: Vec<AbsolutePathBuf>) -> Vec<AbsolutePathBuf> {
    let mut resolved_roots = Vec::new();
    for root in workspace_roots {
        if !resolved_roots.iter().any(|existing| existing == &root) {
            resolved_roots.push(root);
        }
    }
    resolved_roots
}

mod config_errors;
mod request_errors;
mod thread_delete;
mod thread_goal_processor;
mod thread_lifecycle;
mod thread_resume_redaction;
mod thread_summary;

use self::config_errors::*;
use self::request_errors::*;
use self::thread_goal_processor::api_thread_goal_from_state;
use self::thread_lifecycle::*;
use self::thread_resume_redaction::*;
use self::thread_summary::*;

pub(crate) use self::thread_lifecycle::populate_thread_turns_from_history;
pub(crate) use self::thread_processor::thread_from_stored_thread;
#[cfg(test)]
pub(crate) use self::thread_summary::read_summary_from_rollout;
#[cfg(test)]
pub(crate) use self::thread_summary::summary_to_thread;
pub(crate) use self::thread_summary::thread_settings_from_config_snapshot;

pub(crate) fn build_legacy_api_turns_from_rollout_items(items: &[RolloutItem]) -> Vec<Turn> {
    let mut builder = ThreadHistoryBuilder::new();
    for item in items {
        if is_persisted_rollout_item(item, dex_protocol::protocol::ThreadHistoryMode::Legacy) {
            builder.handle_rollout_item(item);
        }
    }
    builder.finish()
}
