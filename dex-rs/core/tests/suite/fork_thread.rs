use std::sync::Arc;

use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::submit_thread_settings;
use core_test_support::test_dex::test_dex;
use core_test_support::wait_for_event;
use dex_core::ForkSnapshot;
use dex_core::NewThread;
use dex_core::TurnInputRequest;
use dex_core::parse_turn_item;
use dex_history::InitialHistory;
use dex_history::ResumedHistory;
use dex_history::RolloutItem;
use dex_protocol::ThreadId;
use dex_protocol::items::TurnItem;
use dex_protocol::mcp::ClientMcpExtensions;
use dex_protocol::protocol::EventMsg;
use dex_protocol::protocol::ThreadHistoryMode;
use dex_protocol::protocol::ThreadSettingsAppliedEvent;
use dex_protocol::protocol::ThreadSettingsOverrides;
use dex_protocol::protocol::ThreadSettingsSnapshot;
use dex_protocol::user_input::UserInput;
use dex_thread_store::InMemoryThreadStore;
use pretty_assertions::assert_eq;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ephemeral_fork_skips_stored_title_lookup() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let store = Arc::new(InMemoryThreadStore::default());
    let test = test_dex()
        .with_thread_store(store.clone())
        .build_with_auto_env(&server)
        .await?;
    let mut config = test.config.clone();
    config.ephemeral = true;
    let reads_before = store.calls().await.read_thread;

    test.thread_manager
        .fork_thread_from_history(
            ForkSnapshot::Interrupted,
            dex_core::StartThreadOptions::new(config),
            InitialHistory::Resumed(ResumedHistory {
                history_revision: None,
                conversation_id: test.session_configured.thread_id,
                history: Arc::new(Vec::new()),
                rollout_path: None,
            }),
        )
        .await?;

    assert_eq!(store.calls().await.read_thread, reads_before);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fork_thread_twice_drops_to_first_message() {
    skip_if_no_network!();

    // Start a mock server that completes three turns.
    let server = MockServer::start().await;
    let sse = sse(vec![ev_response_created("resp"), ev_completed("resp")]);
    let first = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(sse.clone(), "text/event-stream");

    // Expect three calls to /v1/responses – one per user input.
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(first)
        .expect(3)
        .mount(&server)
        .await;

    let mut builder = test_dex();
    let test = builder.build(&server).await.expect("create conversation");
    let dex = test.dex.clone();
    let thread_manager = test.thread_manager.clone();
    let config_for_fork = test.config.clone();

    // Send three user messages; wait for three completed turns.
    for text in ["first", "second", "third"] {
        dex.start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }]))
        .await
        .unwrap();
        let _ = wait_for_event(&dex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
    }

    // Request history from the base conversation to obtain rollout path.
    let base_path = dex.rollout_path().expect("rollout path");

    // GetHistory flushes before returning the path; no wait needed.

    // Compute expected prefixes after each fork by truncating base rollout
    // strictly before the nth user input (0-based).
    let base_items = read_rollout_items(&base_path);
    let find_user_input_positions = |items: &[RolloutItem]| -> Vec<usize> {
        let mut pos = Vec::new();
        for (i, it) in items.iter().enumerate() {
            if let RolloutItem::ResponseItem(response_item) = it
                && let Some(TurnItem::UserMessage(_)) = parse_turn_item(response_item)
            {
                // Consider any user message as an input boundary; recorder stores both EventMsg and ResponseItem.
                // We specifically look for input items, which are represented as ContentItem::InputText.
                pos.push(i);
            }
        }
        pos
    };
    let user_inputs = find_user_input_positions(&base_items);

    // After cutting at nth user input (n=1 → second user message), cut strictly before that input.
    let cut1 = user_inputs.get(1).copied().unwrap_or(0);
    let mut expected_after_first: Vec<RolloutItem> = base_items[..cut1].to_vec();

    // After dropping again (n=1 on fork1), compute expected relative to fork1's rollout.

    // Fork once with n=1 → drops the last user input and everything after.
    let NewThread {
        thread_id: fork1_thread_id,
        thread: dex_fork1,
        ..
    } = thread_manager
        .fork_legacy_thread(
            ForkSnapshot::TruncateBeforeNthUserMessage(1),
            dex_core::StartThreadOptions::new(config_for_fork.clone()),
            base_path.clone(),
        )
        .await
        .expect("fork 1");

    let fork1_path = dex_fork1.rollout_path().expect("rollout path");
    expected_after_first.push(thread_settings_applied_item(
        fork1_thread_id,
        dex_fork1.thread_settings_snapshot().await,
    ));

    // GetHistory on fork1 flushed; the file is ready.
    let fork1_items = read_rollout_items(&fork1_path);
    pretty_assertions::assert_eq!(
        serde_json::to_value(&fork1_items).unwrap(),
        serde_json::to_value(&expected_after_first).unwrap()
    );

    // Fork again with n=0 → drops the (new) last user message, leaving only the first.
    let NewThread {
        thread_id: fork2_thread_id,
        thread: dex_fork2,
        ..
    } = thread_manager
        .fork_legacy_thread(
            ForkSnapshot::TruncateBeforeNthUserMessage(0),
            dex_core::StartThreadOptions::new(config_for_fork.clone()),
            fork1_path.clone(),
        )
        .await
        .expect("fork 2");

    let fork2_path = dex_fork2.rollout_path().expect("rollout path");
    // GetHistory on fork2 flushed; the file is ready.
    let fork1_items = read_rollout_items(&fork1_path);
    let fork1_user_inputs = find_user_input_positions(&fork1_items);
    let cut_last_on_fork1 = fork1_user_inputs
        .get(fork1_user_inputs.len().saturating_sub(1))
        .copied()
        .unwrap_or(0);
    let mut expected_after_second: Vec<RolloutItem> = fork1_items[..cut_last_on_fork1].to_vec();
    expected_after_second.push(thread_settings_applied_item(
        fork2_thread_id,
        dex_fork2.thread_settings_snapshot().await,
    ));
    let fork2_items = read_rollout_items(&fork2_path);
    pretty_assertions::assert_eq!(
        serde_json::to_value(&fork2_items).unwrap(),
        serde_json::to_value(&expected_after_second).unwrap()
    );
}

fn thread_settings_applied_item(
    thread_id: ThreadId,
    snapshot: ThreadSettingsSnapshot,
) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(
        ThreadSettingsAppliedEvent {
            thread_id: Some(thread_id),
            thread_settings: snapshot,
        },
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fork_thread_restores_history_selection_and_preserves_explicit_clear() -> anyhow::Result<()>
{
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let mut builder = test_dex();
    let test = builder.build_with_auto_env(&server).await?;
    let selected = vec!["slack@openai".to_string()];
    submit_thread_settings(
        &test.dex,
        ThreadSettingsOverrides {
            disabled_plugin_ids: Some(selected.clone()),
            ..Default::default()
        },
    )
    .await?;
    let history = InitialHistory::Resumed(ResumedHistory {
        history_revision: None,
        conversation_id: test.session_configured.thread_id,
        history: Arc::new(vec![thread_settings_applied_item(
            test.session_configured.thread_id,
            test.dex.thread_settings_snapshot().await,
        )]),
        rollout_path: None,
    });
    submit_thread_settings(
        &test.dex,
        ThreadSettingsOverrides {
            disabled_plugin_ids: Some(Vec::new()),
            ..Default::default()
        },
    )
    .await?;

    let inherited = test
        .thread_manager
        .fork_thread_from_history(
            ForkSnapshot::Interrupted,
            dex_core::StartThreadOptions::new(test.config.clone()),
            history.clone(),
        )
        .await?;
    pretty_assertions::assert_eq!(
        inherited
            .thread
            .thread_settings_snapshot()
            .await
            .disabled_plugin_ids,
        selected
    );

    let explicit = test
        .thread_manager
        .fork_thread_from_history(
            ForkSnapshot::Interrupted,
            dex_core::StartThreadOptions {
                disabled_plugin_ids: Some(Vec::new()),
                ..dex_core::StartThreadOptions::new(test.config.clone())
            },
            history,
        )
        .await?;
    pretty_assertions::assert_eq!(
        explicit
            .thread
            .thread_settings_snapshot()
            .await
            .disabled_plugin_ids,
        Vec::<String>::new()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fork_thread_from_history_does_not_require_source_rollout_path() {
    assert_copied_fork_persists_inherited_history(ThreadHistoryMode::Legacy).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copied_paginated_fork_persists_inherited_history() {
    assert_copied_fork_persists_inherited_history(ThreadHistoryMode::Paginated).await;
}

async fn assert_copied_fork_persists_inherited_history(history_mode: ThreadHistoryMode) {
    skip_if_no_network!();

    let server = MockServer::start().await;
    let sse = sse(vec![ev_response_created("resp"), ev_completed("resp")]);
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(sse, "text/event-stream"),
        )
        .expect(if matches!(history_mode, ThreadHistoryMode::Paginated) {
            2
        } else {
            1
        })
        .mount(&server)
        .await;

    let mut builder = test_dex().with_history_mode(history_mode);
    let test = builder.build(&server).await.expect("create conversation");
    let dex = test.dex.clone();
    let thread_manager = test.thread_manager.clone();

    dex.start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
        text: "fork me from stored history".to_string(),
        text_elements: Vec::new(),
    }]))
    .await
    .expect("submit initial user turn");
    let _ = wait_for_event(&dex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let source_path = dex.rollout_path().expect("source rollout path");
    let source_items = read_rollout_items(&source_path);
    let source_meta = dex_rollout::read_session_meta_line(source_path.as_path())
        .await
        .expect("read source session metadata");
    let mut supplied_history = vec![RolloutItem::SessionMeta(Box::new(source_meta))];
    supplied_history.extend(source_items.iter().cloned());
    let NewThread {
        thread: forked_thread,
        ..
    } = thread_manager
        .fork_thread_from_history(
            ForkSnapshot::Interrupted,
            dex_core::StartThreadOptions::new(test.config.clone()),
            InitialHistory::Resumed(ResumedHistory {
                history_revision: None,
                conversation_id: test.session_configured.thread_id,
                history: Arc::new(supplied_history),
                rollout_path: None,
            }),
        )
        .await
        .expect("fork from stored history");

    let forked_path = forked_thread.rollout_path().expect("forked rollout path");
    let forked_items = read_rollout_items(&forked_path);
    let forked_items = forked_items
        .iter()
        .map(|item| serde_json::to_value(item).expect("serialize forked rollout item"))
        .collect::<Vec<_>>();
    let source_items = source_items
        .iter()
        .map(|item| serde_json::to_value(item).expect("serialize source rollout item"))
        .collect::<Vec<_>>();
    assert!(
        forked_items.starts_with(&source_items),
        "forked history should start with the supplied source history"
    );

    if matches!(history_mode, ThreadHistoryMode::Paginated) {
        forked_thread
            .shutdown_and_wait()
            .await
            .expect("shutdown copied paginated fork");
        let resumed_history = dex_rollout::RolloutRecorder::get_rollout_history(&forked_path)
            .await
            .expect("load copied paginated fork history");
        let resumed = thread_manager
            .resume_thread_with_history(
                test.config.clone(),
                resumed_history,
                dex_core::test_support::auth_manager_from_auth(dex_login::DexAuth::from_api_key(
                    "dummy",
                )),
                /*parent_trace*/ None,
                ClientMcpExtensions::default(),
            )
            .await
            .expect("resume copied paginated fork")
            .thread;
        resumed
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "continue after cold resume".to_string(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("start resumed turn");
        wait_for_event(&resumed, |event| matches!(event, EventMsg::TurnComplete(_))).await;
        let requests = server.received_requests().await.expect("response requests");
        let input = serde_json::to_string(
            &requests
                .last()
                .expect("resumed model request")
                .body_json::<serde_json::Value>()
                .expect("response request body")["input"],
        )
        .expect("serialize model input");
        assert!(input.contains("fork me from stored history"));
        assert!(input.contains("continue after cold resume"));
    }
}

fn read_rollout_items(path: &std::path::Path) -> Vec<RolloutItem> {
    let read_message = format!("failed to read rollout file {}", path.display());
    let text = std::fs::read_to_string(path).expect(&read_message);
    let mut items: Vec<RolloutItem> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parse_json_message = format!("failed to parse rollout JSON line `{line}`");
        let v: serde_json::Value = serde_json::from_str(line).expect(&parse_json_message);
        let parse_line_message = format!("failed to parse rollout line `{line}`");
        let rl = dex_rollout::decode_rollout_line(v).expect(&parse_line_message);
        match rl.item {
            RolloutItem::SessionMeta(_) => {}
            other => items.push(other),
        }
    }
    items
}
