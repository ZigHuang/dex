use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::path::PathBuf;

use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use dex_config::types::AuthCredentialsStoreMode;
use dex_core::spawn::DEX_SANDBOX_NETWORK_DISABLED_ENV_VAR;
use dex_mcp_server::DexToolCallParam;
use dex_mcp_server::ExecApprovalElicitRequestParams;
use dex_mcp_server::ExecApprovalResponse;
use dex_mcp_server::PatchApprovalElicitRequestParams;
use dex_mcp_server::PatchApprovalResponse;
use dex_protocol::protocol::FileChange;
use dex_protocol::protocol::ReviewDecision;
use dex_shell_command::parse_command;
use pretty_assertions::assert_eq;
use rmcp::model::JsonRpcResponse;
use rmcp::model::JsonRpcVersion2_0;
use rmcp::model::RequestId;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

use core_test_support::skip_if_no_network;
use mcp_test_support::McpProcess;
use mcp_test_support::create_apply_patch_sse_response;
use mcp_test_support::create_command_execution_sse_response;
use mcp_test_support::create_final_assistant_message_sse_response;
use mcp_test_support::create_mock_responses_server;
use mcp_test_support::format_with_current_shell;

// Windows CI can spend tens of seconds in session startup before the first
// mock model request is sent.
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Test that an explicitly escalated exec command triggers MCP elicitation and
/// that approving the request runs the command.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_exec_command_approval_triggers_elicitation() {
    if env::var(DEX_SANDBOX_NETWORK_DISABLED_ENV_VAR).is_ok() {
        println!(
            "Skipping test because it cannot execute when network is disabled in a Dex sandbox."
        );
        return;
    }

    // Apparently `#[tokio::test]` must return `()`, so we create a helper
    // function that returns `Result` so we can use `?` in favor of `unwrap`.
    exec_command_approval_triggers_elicitation()
        .await
        .expect("exec command approval should trigger elicitation");
}

async fn exec_command_approval_triggers_elicitation() -> anyhow::Result<()> {
    // Use a command that creates a file so we can observe its side effect.
    let workdir_for_shell_function_call = TempDir::new()?;
    let created_filename = "created_by_shell_tool.txt";
    let created_file = workdir_for_shell_function_call
        .path()
        .join(created_filename);

    let (shell_command, timeout_ms) = if cfg!(windows) {
        (
            vec![
                "New-Item".to_string(),
                "-ItemType".to_string(),
                "File".to_string(),
                "-Path".to_string(),
                created_filename.to_string(),
                "-Force".to_string(),
            ],
            // `powershell.exe` startup can be slow on loaded Windows CI workers
            10_000,
        )
    } else {
        (
            vec!["touch".to_string(), created_filename.to_string()],
            5_000,
        )
    };
    let expected_shell_command =
        format_with_current_shell(&shlex::try_join(shell_command.iter().map(String::as_str))?);

    let McpHandle {
        process: mut mcp_process,
        server: _server,
        dir: _dir,
    } = create_mcp_process(vec![
        create_command_execution_sse_response(
            shell_command.clone(),
            Some(workdir_for_shell_function_call.path()),
            Some(timeout_ms),
            "call1234",
        )?,
        create_final_assistant_message_sse_response("File created!")?,
    ])
    .await?;

    // Send a "dex" tool request, which should hit the responses endpoint.
    // In turn, it should reply with a tool call, which the MCP should forward
    // as an elicitation.
    let dex_request_id = mcp_process
        .send_dex_tool_call(DexToolCallParam {
            prompt: "run `git init`".to_string(),
            // Exercise MCP elicitation even when the surrounding environment enables auto-review.
            config: Some(HashMap::from([
                ("approvals_reviewer".to_string(), json!("user")),
                ("features.guardian_approval".to_string(), json!(false)),
            ])),
            ..Default::default()
        })
        .await?;
    let elicitation_request = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp_process.read_stream_until_request_message(),
    )
    .await??;

    assert_eq!(elicitation_request.jsonrpc, JsonRpcVersion2_0);
    assert_eq!(elicitation_request.request.method, "elicitation/create");

    let elicitation_request_id = elicitation_request.id.clone();
    let params = serde_json::from_value::<ExecApprovalElicitRequestParams>(
        elicitation_request
            .request
            .params
            .clone()
            .ok_or_else(|| anyhow::anyhow!("elicitation_request.params must be set"))?,
    )?;
    assert_eq!(
        elicitation_request.request.params,
        Some(create_expected_elicitation_request_params(
            expected_shell_command,
            workdir_for_shell_function_call.path(),
            dex_request_id.to_string(),
            params.dex_event_id.clone(),
            params.thread_id,
        )?)
    );

    // Accept the `git init` request by responding to the elicitation.
    mcp_process
        .send_response(
            elicitation_request_id,
            serde_json::to_value(ExecApprovalResponse {
                decision: ReviewDecision::Approved,
            })?,
        )
        .await?;

    // Verify task_complete notification arrives before the tool call completes.
    let _task_complete = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp_process.read_stream_until_legacy_task_complete_notification(),
    )
    .await
    .expect("task_complete_notification timeout")
    .expect("task_complete_notification resp");

    // Verify the original `dex` tool call completes and that the file was created.
    let dex_response = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp_process.read_stream_until_response_message(RequestId::Number(dex_request_id)),
    )
    .await??;
    assert_eq!(
        JsonRpcResponse {
            jsonrpc: JsonRpcVersion2_0,
            id: RequestId::Number(dex_request_id),
            result: json!({
                "content": [
                    {
                        "text": "File created!",
                        "type": "text"
                    }
                ],
                "structuredContent": {
                    "threadId": params.thread_id,
                    "content": "File created!"
                }
            }),
        },
        dex_response
    );

    assert!(created_file.is_file(), "created file should exist");

    Ok(())
}

fn create_expected_elicitation_request_params(
    command: Vec<String>,
    workdir: &Path,
    dex_mcp_tool_call_id: String,
    dex_event_id: String,
    thread_id: dex_protocol::ThreadId,
) -> anyhow::Result<serde_json::Value> {
    let expected_message = format!(
        "Allow Dex to run `{}` in `{}`?",
        shlex::try_join(command.iter().map(std::convert::AsRef::as_ref))?,
        workdir.to_string_lossy()
    );
    let dex_parsed_cmd = parse_command::parse_command(&command);
    let params_json = serde_json::to_value(ExecApprovalElicitRequestParams {
        message: expected_message,
        requested_schema: json!({"type":"object","properties":{}}),
        thread_id,
        dex_elicitation: "exec-approval".to_string(),
        dex_mcp_tool_call_id,
        dex_event_id,
        dex_command: command,
        dex_cwd: workdir.to_path_buf(),
        dex_call_id: "call1234".to_string(),
        dex_parsed_cmd,
    })?;
    Ok(params_json)
}

/// Test that patch approval triggers an elicitation request to the MCP and that
/// sending a denial leaves the patch unapplied, as expected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_patch_approval_triggers_elicitation() {
    if env::var(DEX_SANDBOX_NETWORK_DISABLED_ENV_VAR).is_ok() {
        println!(
            "Skipping test because it cannot execute when network is disabled in a Dex sandbox."
        );
        return;
    }

    patch_approval_triggers_elicitation()
        .await
        .expect("patch approval should trigger elicitation");
}

async fn patch_approval_triggers_elicitation() -> anyhow::Result<()> {
    if cfg!(windows) {
        // powershell apply_patch shell calls are not parsed into apply patch approvals

        return Ok(());
    }

    let cwd = TempDir::new()?;
    let test_file = Path::new("/").join(
        cwd.path()
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("temporary directory must have a file name"))?,
    );

    let patch_content = format!(
        "*** Begin Patch\n*** Add File: {}\n+new content\n*** End Patch",
        test_file.as_path().to_string_lossy()
    );

    let McpHandle {
        process: mut mcp_process,
        server: _server,
        dir: _dir,
    } = create_mcp_process(vec![
        create_apply_patch_sse_response(&patch_content, "call1234")?,
        create_final_assistant_message_sse_response("Patch was not applied.")?,
    ])
    .await?;

    // Send a "dex" tool request that will trigger the apply_patch command
    let dex_request_id = mcp_process
        .send_dex_tool_call(DexToolCallParam {
            cwd: Some(cwd.path().to_string_lossy().to_string()),
            prompt: "please modify the test file".to_string(),
            // Use the user reviewer so this test exercises MCP elicitation even when auto-review
            // is enabled by the surrounding environment.
            config: Some(HashMap::from([
                ("approvals_reviewer".to_string(), json!("user")),
                ("features.guardian_approval".to_string(), json!(false)),
            ])),
            ..Default::default()
        })
        .await?;
    let elicitation_request = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp_process.read_stream_until_request_message(),
    )
    .await??;

    assert_eq!(elicitation_request.jsonrpc, JsonRpcVersion2_0);
    assert_eq!(elicitation_request.request.method, "elicitation/create");

    let elicitation_request_id = elicitation_request.id.clone();
    let params = serde_json::from_value::<PatchApprovalElicitRequestParams>(
        elicitation_request
            .request
            .params
            .clone()
            .ok_or_else(|| anyhow::anyhow!("elicitation_request.params must be set"))?,
    )?;

    let mut expected_changes = HashMap::new();
    expected_changes.insert(
        test_file.as_path().to_path_buf(),
        FileChange::Add {
            content: "new content\n".to_string(),
        },
    );

    assert_eq!(
        elicitation_request.request.params,
        Some(create_expected_patch_approval_elicitation_request_params(
            expected_changes,
            /*grant_root*/ None, // No grant_root expected
            /*reason*/ None,
            dex_request_id.to_string(),
            params.dex_event_id.clone(),
            params.thread_id,
        )?)
    );

    // Deny the patch approval request so this test does not require the Linux sandbox helper.
    mcp_process
        .send_response(
            elicitation_request_id,
            serde_json::to_value(PatchApprovalResponse {
                decision: ReviewDecision::denied("rejected by test"),
            })?,
        )
        .await?;

    // Verify the original `dex` tool call completes
    let dex_response = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp_process.read_stream_until_response_message(RequestId::Number(dex_request_id)),
    )
    .await??;
    assert_eq!(
        JsonRpcResponse {
            jsonrpc: JsonRpcVersion2_0,
            id: RequestId::Number(dex_request_id),
            result: json!({
                "content": [
                    {
                        "text": "Patch was not applied.",
                        "type": "text"
                    }
                ],
                "structuredContent": {
                    "threadId": params.thread_id,
                    "content": "Patch was not applied."
                }
            }),
        },
        dex_response
    );

    assert!(!test_file.exists());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dex_tool_passes_base_instructions() {
    skip_if_no_network!();

    // Apparently `#[tokio::test]` must return `()`, so we create a helper
    // function that returns `Result` so we can use `?` in favor of `unwrap`.
    dex_tool_passes_base_instructions()
        .await
        .expect("dex tool should pass base instructions");
}

async fn dex_tool_passes_base_instructions() -> anyhow::Result<()> {
    #![expect(clippy::unwrap_used)]

    let server =
        create_mock_responses_server(vec![create_final_assistant_message_sse_response("Enjoy!")?])
            .await;
    let caller_server = MockServer::start().await;

    // Run `dex mcp` with a specific config.toml.
    let dex_home = TempDir::new()?;
    create_config_toml(dex_home.path(), &server.uri())?;
    let skill_dir = dex_home.path().join("skills").join("demo");
    std::fs::create_dir_all(&skill_dir)?;
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: Demo skill.\n---\n# Demo\n\nUse this skill.\n",
    )?;
    write_chatgpt_auth(
        dex_home.path(),
        ChatGptAuthFixture::new("chatgpt-token").account_id("workspace-123"),
        AuthCredentialsStoreMode::File,
    )?;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/settings/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "commit_attribution_enabled": true,
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut mcp_process = McpProcess::new_with_env(
        dex_home.path(),
        &[("OPENAI_API_KEY", None), ("DEX_ACCESS_TOKEN", None)],
    )
    .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp_process.initialize()).await??;

    // Send a "dex" tool request, which should hit the responses endpoint.
    let dex_request_id = mcp_process
        .send_dex_tool_call(DexToolCallParam {
            prompt: "How are you?".to_string(),
            config: Some(HashMap::from([(
                "chatgpt_base_url".to_string(),
                json!(format!("{}/backend-api", caller_server.uri())),
            )])),
            base_instructions: Some("You are a helpful assistant.".to_string()),
            developer_instructions: Some("Foreshadow upcoming tool calls.".to_string()),
            ..Default::default()
        })
        .await?;

    let dex_response = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp_process.read_stream_until_response_message(RequestId::Number(dex_request_id)),
    )
    .await??;
    assert_eq!(dex_response.jsonrpc, JsonRpcVersion2_0);
    assert_eq!(dex_response.id, RequestId::Number(dex_request_id));
    assert_eq!(
        dex_response.result,
        json!({
            "content": [
                {
                    "text": "Enjoy!",
                    "type": "text"
                }
            ],
            "structuredContent": {
                "threadId": dex_response
                    .result
                    .get("structuredContent")
                    .and_then(|v| v.get("threadId"))
                    .and_then(serde_json::Value::as_str)
                    .expect("dex tool response should include structuredContent.threadId"),
                "content": "Enjoy!"
            }
        })
    );

    let requests = server.received_requests().await.unwrap();
    let request = requests
        .iter()
        .find(|request| request.url.path() == "/v1/responses")
        .expect("mock model request should be recorded")
        .body_json::<serde_json::Value>()?;
    let instructions = request["instructions"]
        .as_str()
        .expect("responses request should include instructions");
    assert!(instructions.starts_with("You are a helpful assistant."));
    let developer_messages: Vec<&serde_json::Value> = request["input"]
        .as_array()
        .expect("responses request should include input items")
        .iter()
        .filter(|msg| msg.get("role").and_then(|role| role.as_str()) == Some("developer"))
        .collect();
    let developer_contents: Vec<&str> = developer_messages
        .iter()
        .filter_map(|msg| msg.get("content").and_then(serde_json::Value::as_array))
        .flat_map(|content| content.iter())
        .filter(|span| span.get("type").and_then(serde_json::Value::as_str) == Some("input_text"))
        .filter_map(|span| span.get("text").and_then(serde_json::Value::as_str))
        .collect();
    let developer_text = developer_contents.join("\n");
    assert_eq!(
        developer_text
            .matches("Co-authored-by: Dex <noreply@openai.com>")
            .count(),
        1
    );
    assert_eq!(
        developer_text
            .matches("Generated with [Dex](https://openai.com/dex/).")
            .count(),
        1
    );
    assert_eq!(
        developer_text.matches("- demo: Demo skill.").count(),
        1,
        "host skill catalog should be included exactly once"
    );
    assert!(
        developer_contents
            .iter()
            .any(|content| content.contains("`sandbox_mode`")),
        "expected permissions developer message, got {developer_contents:?}"
    );
    assert!(
        developer_contents.contains(&"Foreshadow upcoming tool calls."),
        "expected developer instructions in developer messages, got {developer_contents:?}"
    );
    let caller_requests = caller_server.received_requests().await.unwrap();
    assert!(
        caller_requests
            .iter()
            .all(|request| request.url.path() != "/backend-api/wham/settings/user"),
        "attribution settings must use the process-level base URL"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dex_tool_forwards_skills_extension_warnings() {
    skip_if_no_network!();

    dex_tool_forwards_skills_extension_warnings()
        .await
        .expect("dex tool should forward skills extension warnings");
}

async fn dex_tool_forwards_skills_extension_warnings() -> anyhow::Result<()> {
    let server =
        create_mock_responses_server(vec![create_final_assistant_message_sse_response("Enjoy!")?])
            .await;
    let dex_home = TempDir::new()?;
    create_config_toml(dex_home.path(), &server.uri())?;
    let skills_dir = dex_home.path().join("skills");
    for index in 0..200 {
        let name = format!("skill-{index:03}");
        let skill_dir = skills_dir.join(&name);
        std::fs::create_dir_all(&skill_dir)?;
        let description = format!("Skill {index}: {}", "x".repeat(200));
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: {description}\n---\n# {name}\n\nUse this skill.\n"
            ),
        )?;
    }
    let mut mcp_process = McpProcess::new(dex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp_process.initialize()).await??;

    let dex_request_id = mcp_process
        .send_dex_tool_call(DexToolCallParam {
            prompt: "How are you?".to_string(),
            ..Default::default()
        })
        .await?;

    let warning = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp_process.read_stream_until_dex_event_matching("warning", |params| {
            params["msg"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("skills context budget"))
        }),
    )
    .await??;
    let warning_json = serde_json::to_value(&warning)?;
    let params = warning
        .notification
        .params
        .ok_or_else(|| anyhow::anyhow!("warning notification should include params"))?;
    assert_eq!(warning_json["params"]["_meta"]["requestId"], dex_request_id);
    assert!(
        warning_json["params"]["id"]
            .as_str()
            .is_some_and(|turn_id| !turn_id.is_empty())
    );
    assert!(
        warning_json["params"]["_meta"]["threadId"]
            .as_str()
            .is_some_and(|thread_id| !thread_id.is_empty())
    );
    assert_eq!(params["msg"]["type"], "warning");
    assert!(
        params["msg"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("skills context budget"))
    );
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp_process.read_stream_until_response_message(RequestId::Number(dex_request_id)),
    )
    .await??;

    Ok(())
}

fn create_expected_patch_approval_elicitation_request_params(
    changes: HashMap<PathBuf, FileChange>,
    grant_root: Option<PathBuf>,
    reason: Option<String>,
    dex_mcp_tool_call_id: String,
    dex_event_id: String,
    thread_id: dex_protocol::ThreadId,
) -> anyhow::Result<serde_json::Value> {
    let mut message_lines = Vec::new();
    if let Some(r) = &reason {
        message_lines.push(r.clone());
    }
    message_lines.push("Allow Dex to apply proposed code changes?".to_string());
    let params_json = serde_json::to_value(PatchApprovalElicitRequestParams {
        message: message_lines.join("\n"),
        requested_schema: json!({"type":"object","properties":{}}),
        thread_id,
        dex_elicitation: "patch-approval".to_string(),
        dex_mcp_tool_call_id,
        dex_event_id,
        dex_reason: reason,
        dex_grant_root: grant_root,
        dex_changes: changes,
        dex_call_id: "call1234".to_string(),
    })?;

    Ok(params_json)
}

/// This handle is used to ensure that the MockServer and TempDir are not dropped while
/// the McpProcess is still running.
pub struct McpHandle {
    pub process: McpProcess,
    /// Retain the server for the lifetime of the McpProcess.
    #[allow(dead_code)]
    server: MockServer,
    /// Retain the temporary directory for the lifetime of the McpProcess.
    #[allow(dead_code)]
    dir: TempDir,
}

async fn create_mcp_process(responses: Vec<String>) -> anyhow::Result<McpHandle> {
    let server = create_mock_responses_server(responses).await;
    let dex_home = TempDir::new()?;
    create_config_toml(dex_home.path(), &server.uri())?;
    let mut mcp_process = McpProcess::new(dex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp_process.initialize()).await??;
    Ok(McpHandle {
        process: mcp_process,
        server,
        dir: dex_home,
    })
}

/// Create a Dex config that uses the mock server as the model provider.
/// The command explicitly requests escalation so that we exercise the
/// elicitation code path.
fn create_config_toml(dex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    let config_toml = dex_home.join("config.toml");
    std::fs::write(
        config_toml,
        format!(
            r#"
model = "mock-model"
approval_policy = "on-request"
sandbox_policy = "workspace-write"

model_provider = "mock_provider"
chatgpt_base_url = "{server_uri}/backend-api"
cli_auth_credentials_store = "file"

[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "{server_uri}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0

[features]
"#
        ),
    )
}
