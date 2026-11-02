use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_fake_paginated_rollout;
use app_test_support::create_fake_rollout;
use app_test_support::create_mock_responses_server_repeating_assistant;
use core_test_support::responses;
use dex_app_server_protocol::ClientInfo;
use dex_app_server_protocol::ClientRequest;
use dex_app_server_protocol::JSONRPCError;
use dex_app_server_protocol::RequestId;
use dex_app_server_protocol::ThreadArchiveParams;
use dex_app_server_protocol::ThreadArchiveResponse;
use dex_app_server_protocol::ThreadArchivedNotification;
use dex_app_server_protocol::ThreadHistoryMode;
use dex_app_server_protocol::ThreadLoadedListParams;
use dex_app_server_protocol::ThreadLoadedListResponse;
use dex_app_server_protocol::ThreadReadParams;
use dex_app_server_protocol::ThreadReadResponse;
use dex_app_server_protocol::ThreadResumeParams;
use dex_app_server_protocol::ThreadResumeResponse;
use dex_app_server_protocol::ThreadSource;
use dex_app_server_protocol::ThreadStartParams;
use dex_app_server_protocol::ThreadStartResponse;
use dex_app_server_protocol::ThreadStatus;
use dex_app_server_protocol::ThreadUnarchiveParams;
use dex_app_server_protocol::ThreadUnarchiveResponse;
use dex_app_server_protocol::ThreadUnarchivedNotification;
use dex_app_server_protocol::TurnCompletedNotification;
use dex_app_server_protocol::TurnStartParams;
use dex_app_server_protocol::TurnStartResponse;
use dex_app_server_protocol::UserInput;
use dex_core::ARCHIVED_SESSIONS_SUBDIR;
use dex_core::find_archived_thread_path_by_id_str;
use dex_core::find_thread_path_by_id_str;
use dex_features::Feature;
use dex_protocol::ThreadId;
use dex_state::DirectionalThreadSpawnEdgeStatus;
use dex_state::StateRuntime;
use dex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use tempfile::TempDir;
use tokio::time::timeout;

use super::analytics::mount_analytics_capture;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn body_contains(request: &wiremock::Request, text: &str) -> bool {
    String::from_utf8(request.body.clone())
        .ok()
        .is_some_and(|body| body.contains(text))
}

#[tokio::test]
async fn thread_archive_rejects_owned_unmaterialized_paginated_descendant() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let dex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(dex_home.path())?;
    let parent_id = create_fake_rollout(
        dex_home.path(),
        "2025-01-01T00-00-00",
        "2025-01-01T00:00:00Z",
        "parent",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let parent_thread_id = ThreadId::from_string(&parent_id)?;
    let mut owner = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .build_initialized()
        .await?;
    let ThreadStartResponse { thread: child, .. } = owner
        .start_thread(ThreadStartParams {
            history_mode: Some(ThreadHistoryMode::Paginated),
            ..Default::default()
        })
        .await?;
    let child_thread_id = ThreadId::from_string(&child.id)?;
    let state_db = StateRuntime::init(
        dex_state::SqliteConfig::new_for_testing(dex_home.path().abs()),
        "mock_provider".into(),
    )
    .await?;
    state_db
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await?;

    let mut other = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .build_initialized()
        .await?;
    let request_id = other
        .send_thread_archive_request(ThreadArchiveParams {
            thread_id: parent_id.clone(),
        })
        .await?;
    let error: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        other.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_eq!(error.error.code, -32600);
    assert_eq!(
        error.error.message,
        format!("thread {} already has an active writer", child.id)
    );
    timeout(DEFAULT_READ_TIMEOUT, owner.shutdown_gracefully()).await??;
    let _: ThreadArchiveResponse = other
        .request(|request_id| ClientRequest::ThreadArchive {
            request_id,
            params: ThreadArchiveParams {
                thread_id: parent_id,
            },
        })
        .await?;
    Ok(())
}

#[tokio::test]
async fn thread_archive_shuts_down_resumed_archived_descendant() -> Result<()> {
    const RESUME_PROMPT: &str = "resume the child";
    const RESUME_CALL_ID: &str = "resume-call-1";

    let server = responses::start_mock_server().await;
    let dex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::Collab)
        .write(dex_home.path())?;
    let parent_id = create_fake_paginated_rollout(
        dex_home.path(),
        "2025-01-01T00-00-00",
        "2025-01-01T00:00:00Z",
        "parent",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let child_id = create_fake_paginated_rollout(
        dex_home.path(),
        "2025-01-01T00-01-00",
        "2025-01-01T00:01:00Z",
        "child",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let parent_thread_id = ThreadId::from_string(&parent_id)?;
    let child_thread_id = ThreadId::from_string(&child_id)?;
    let state_db = StateRuntime::init(
        dex_state::SqliteConfig::new_for_testing(dex_home.path().abs()),
        "mock_provider".into(),
    )
    .await?;
    state_db
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Closed,
        )
        .await?;

    let mut mcp = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .build_initialized()
        .await?;

    let _: ThreadArchiveResponse = mcp
        .request(|request_id| ClientRequest::ThreadArchive {
            request_id,
            params: ThreadArchiveParams {
                thread_id: child_id.clone(),
            },
        })
        .await?;
    let _: ThreadResumeResponse = mcp
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: parent_id.clone(),
                model: Some("gpt-5.4".to_string()),
                ..Default::default()
            },
        })
        .await?;

    let resume_args = serde_json::to_string(&json!({ "id": child_thread_id }))?;
    let _parent_resume = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, RESUME_PROMPT) && !body_contains(request, RESUME_CALL_ID)
        },
        responses::sse(vec![
            responses::ev_response_created("resp-parent-resume"),
            responses::ev_function_call_with_namespace(
                RESUME_CALL_ID,
                "multi_agent_v1",
                "resume_agent",
                &resume_args,
            ),
            responses::ev_completed("resp-parent-resume"),
        ]),
    )
    .await;
    let _parent_resume_follow_up = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, RESUME_CALL_ID),
        responses::sse(vec![
            responses::ev_response_created("resp-parent-resume-follow-up"),
            responses::ev_assistant_message("msg-parent-resume", "parent done"),
            responses::ev_completed("resp-parent-resume-follow-up"),
        ]),
    )
    .await;

    let _: TurnStartResponse = mcp
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: parent_id.clone(),
                input: vec![UserInput::Text {
                    text: RESUME_PROMPT.to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        })
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, async {
        loop {
            let completed: TurnCompletedNotification =
                mcp.read_notification("turn/completed").await?;
            if completed.thread_id == parent_id {
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await??;

    let ThreadLoadedListResponse { data, .. } = mcp
        .request(|request_id| ClientRequest::ThreadLoadedList {
            request_id,
            params: ThreadLoadedListParams::default(),
        })
        .await?;
    assert!(data.contains(&child_id));

    let _: ThreadArchiveResponse = mcp
        .request(|request_id| ClientRequest::ThreadArchive {
            request_id,
            params: ThreadArchiveParams {
                thread_id: parent_id.clone(),
            },
        })
        .await?;
    let ThreadLoadedListResponse { data, .. } = mcp
        .request(|request_id| ClientRequest::ThreadLoadedList {
            request_id,
            params: ThreadLoadedListParams::default(),
        })
        .await?;
    assert_eq!(data, Vec::<String>::new());

    unarchive(&mut mcp, &parent_id).await?;
    assert!(
        find_thread_path_by_id_str(dex_home.path(), &child_id, /*state_db_ctx*/ None)
            .await?
            .is_some(),
        "restoring the parent should also restore the independently archived child"
    );

    Ok(())
}

#[test_case::test_case(ThreadHistoryMode::Legacy; "legacy")]
#[test_case::test_case(ThreadHistoryMode::Paginated; "paginated")]
#[tokio::test]
async fn thread_archive_without_turns(history_mode: ThreadHistoryMode) -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let dex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_root_config(&format!(r#"chatgpt_base_url = "{}""#, server.uri()))
        .write(dex_home.path())?;
    mount_analytics_capture(&server, dex_home.path()).await?;

    let mut mcp = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .build()
        .await?;
    mcp.initialize_with_client_info(ClientInfo {
        name: "dex_work_desktop".to_string(),
        title: None,
        version: "0.1.0".to_string(),
    })
    .await?;

    // Start a thread.
    let ThreadStartResponse { thread, .. } = mcp
        .start_thread(ThreadStartParams {
            model: Some("mock-model".to_string()),
            history_mode: Some(history_mode),
            thread_source: Some(ThreadSource::User),
            service_name: Some("dex_work_desktop".to_string()),
            ..Default::default()
        })
        .await?;
    assert!(!thread.id.is_empty());

    let rollout_path = thread.path.clone().expect("thread path");
    assert!(
        !rollout_path.exists(),
        "fresh thread rollout should not exist yet at {}",
        rollout_path.display()
    );
    assert!(
        find_thread_path_by_id_str(dex_home.path(), &thread.id, /*state_db_ctx*/ None)
            .await?
            .is_none(),
        "thread id should not be discoverable before rollout materialization"
    );

    // Archiving materializes the empty rollout without creating a user turn.
    let _: ThreadArchiveResponse = mcp
        .request(|request_id| ClientRequest::ThreadArchive {
            request_id,
            params: ThreadArchiveParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    let archived_notification: ThreadArchivedNotification = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_notification("thread/archived"),
    )
    .await??;
    assert_eq!(archived_notification.thread_id, thread.id);

    let ThreadReadResponse {
        thread: archived, ..
    } = mcp
        .request(|request_id| ClientRequest::ThreadRead {
            request_id,
            params: ThreadReadParams {
                thread_id: thread.id.clone(),
                include_turns: true,
            },
        })
        .await?;
    assert_eq!(archived.turns, Vec::new());
    assert_eq!(archived.status, ThreadStatus::NotLoaded);

    // Verify file moved.
    let archived_directory = dex_home.path().join(ARCHIVED_SESSIONS_SUBDIR);
    // The archived file keeps the original filename (rollout-...-<id>.jsonl).
    let archived_rollout_path =
        archived_directory.join(rollout_path.file_name().expect("rollout file name"));
    assert!(
        !rollout_path.exists(),
        "expected rollout path {} to be moved",
        rollout_path.display()
    );
    assert!(
        archived_rollout_path.exists(),
        "expected archived rollout path {} to exist",
        archived_rollout_path.display()
    );

    assert_paths_match_on_disk(
        archived.path.as_deref().expect("archived thread path"),
        &archived_rollout_path,
    )?;

    Ok(())
}

#[test_case::test_case(false; "restore the whole subtree")]
#[test_case::test_case(true; "continue past a child restore failure")]
#[tokio::test]
async fn thread_archive_and_unarchive_spawned_descendants(fail_child_restore: bool) -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let dex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(dex_home.path())?;

    let parent_id = create_fake_rollout(
        dex_home.path(),
        "2025-01-01T00-00-00",
        "2025-01-01T00:00:00Z",
        "parent",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let child_id = create_fake_rollout(
        dex_home.path(),
        "2025-01-01T00-01-00",
        "2025-01-01T00:01:00Z",
        "child",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let grandchild_id = create_fake_rollout(
        dex_home.path(),
        "2025-01-01T00-02-00",
        "2025-01-01T00:02:00Z",
        "grandchild",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;

    let parent_thread_id = ThreadId::from_string(&parent_id)?;
    let child_thread_id = ThreadId::from_string(&child_id)?;
    let grandchild_thread_id = ThreadId::from_string(&grandchild_id)?;
    let child_rollout_path =
        find_thread_path_by_id_str(dex_home.path(), &child_id, /*state_db_ctx*/ None)
            .await?
            .expect("child rollout path");
    let state_db = StateRuntime::init(
        dex_state::SqliteConfig::new_for_testing(dex_home.path().abs()),
        "mock_provider".into(),
    )
    .await?;
    state_db
        .mark_backfill_complete(/*last_watermark*/ None)
        .await?;
    state_db
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Closed,
        )
        .await?;
    state_db
        .upsert_thread_spawn_edge(
            child_thread_id,
            grandchild_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await?;

    let mut mcp = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;

    let _: ThreadArchiveResponse = mcp
        .request(|request_id| ClientRequest::ThreadArchive {
            request_id,
            params: ThreadArchiveParams {
                thread_id: parent_id.clone(),
            },
        })
        .await?;

    for thread_id in [&parent_id, &grandchild_id, &child_id] {
        let notification: ThreadArchivedNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_notification("thread/archived"),
        )
        .await??;
        assert_eq!(&notification.thread_id, thread_id);
    }

    for thread_id in [parent_thread_id, child_thread_id, grandchild_thread_id] {
        assert!(
            find_thread_path_by_id_str(
                dex_home.path(),
                &thread_id.to_string(),
                /*state_db_ctx*/ None,
            )
            .await?
            .is_none(),
            "expected active rollout for {thread_id} to be archived"
        );
        assert!(
            find_archived_thread_path_by_id_str(
                dex_home.path(),
                &thread_id.to_string(),
                /*state_db_ctx*/ None,
            )
            .await?
            .is_some(),
            "expected archived rollout for {thread_id} to exist"
        );
    }

    if fail_child_restore {
        std::fs::create_dir_all(&child_rollout_path)?;
    }
    unarchive(&mut mcp, &parent_id).await?;
    let expected = if fail_child_restore {
        vec![&parent_id, &grandchild_id]
    } else {
        vec![&parent_id, &child_id, &grandchild_id]
    };
    for thread_id in expected {
        let notification: ThreadUnarchivedNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_notification("thread/unarchived"),
        )
        .await??;
        assert_eq!(&notification.thread_id, thread_id);
    }
    assert_eq!(
        find_archived_thread_path_by_id_str(dex_home.path(), &child_id, /*state_db_ctx*/ None)
            .await?
            .is_some(),
        fail_child_restore
    );

    Ok(())
}

#[tokio::test]
async fn thread_archive_succeeds_when_descendant_archive_fails() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let dex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_root_config(&format!(r#"chatgpt_base_url = "{}""#, server.uri()))
        .write(dex_home.path())?;
    mount_analytics_capture(&server, dex_home.path()).await?;

    let parent_id = create_fake_rollout(
        dex_home.path(),
        "2025-01-01T00-00-00",
        "2025-01-01T00:00:00Z",
        "parent",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let child_id = create_fake_rollout(
        dex_home.path(),
        "2025-01-01T00-01-00",
        "2025-01-01T00:01:00Z",
        "child",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let grandchild_id = create_fake_rollout(
        dex_home.path(),
        "2025-01-01T00-02-00",
        "2025-01-01T00:02:00Z",
        "grandchild",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;

    let parent_thread_id = ThreadId::from_string(&parent_id)?;
    let child_thread_id = ThreadId::from_string(&child_id)?;
    let grandchild_thread_id = ThreadId::from_string(&grandchild_id)?;
    let state_db = StateRuntime::init(
        dex_state::SqliteConfig::new_for_testing(dex_home.path().abs()),
        "mock_provider".into(),
    )
    .await?;
    state_db
        .mark_backfill_complete(/*last_watermark*/ None)
        .await?;
    state_db
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Closed,
        )
        .await?;
    state_db
        .upsert_thread_spawn_edge(
            child_thread_id,
            grandchild_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await?;

    let child_rollout_path =
        find_thread_path_by_id_str(dex_home.path(), &child_id, /*state_db_ctx*/ None)
            .await?
            .expect("child rollout path");
    let archived_child_path = dex_home
        .path()
        .join(ARCHIVED_SESSIONS_SUBDIR)
        .join(child_rollout_path.file_name().expect("rollout file name"));
    std::fs::create_dir_all(&archived_child_path)?;

    let mut mcp = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;

    let _: ThreadArchiveResponse = mcp
        .request(|request_id| ClientRequest::ThreadArchive {
            request_id,
            params: ThreadArchiveParams {
                thread_id: parent_id.clone(),
            },
        })
        .await?;

    let mut archived_ids = Vec::new();
    for _ in 0..2 {
        let archived_notification: ThreadArchivedNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_notification("thread/archived"),
        )
        .await??;
        archived_ids.push(archived_notification.thread_id);
    }
    assert_eq!(archived_ids, vec![parent_id.clone(), grandchild_id.clone()]);

    assert!(
        timeout(
            std::time::Duration::from_millis(250),
            mcp.read_stream_until_notification_message("thread/archived"),
        )
        .await
        .is_err()
    );

    assert!(
        child_rollout_path.exists(),
        "child should stay active after descendant archive failure"
    );
    assert!(
        archived_child_path.is_dir(),
        "test conflict should remain in archived sessions"
    );
    for thread_id in [parent_thread_id, grandchild_thread_id] {
        assert!(
            find_thread_path_by_id_str(
                dex_home.path(),
                &thread_id.to_string(),
                /*state_db_ctx*/ None,
            )
            .await?
            .is_none(),
            "expected active rollout for {thread_id} to be archived"
        );
        assert!(
            find_archived_thread_path_by_id_str(
                dex_home.path(),
                &thread_id.to_string(),
                /*state_db_ctx*/ None,
            )
            .await?
            .is_some(),
            "expected archived rollout for {thread_id} to exist"
        );
    }

    let repeated_archive_id = mcp
        .send_thread_archive_request(ThreadArchiveParams {
            thread_id: parent_id.clone(),
        })
        .await?;
    let _: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(repeated_archive_id)),
    )
    .await??;

    unarchive(&mut mcp, &parent_id).await?;

    let requests = server
        .received_requests()
        .await
        .ok_or_else(|| anyhow::anyhow!("wiremock did not record requests"))?;
    let mut archive_events = Vec::new();
    for request in requests {
        if request.url.path() != "/dex/analytics-events/events" {
            continue;
        }
        let payload: Value = serde_json::from_slice(&request.body)?;
        let events = payload["events"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("analytics payload missing events array"))?;
        for event in events
            .iter()
            .filter(|event| event["event_type"] == "dex_thread_archive_event")
        {
            for (header, expected) in [
                ("authorization", "Bearer chatgpt-token"),
                ("chatgpt-account-id", "account-123"),
            ] {
                assert_eq!(
                    request
                        .headers
                        .get(header)
                        .and_then(|value| value.to_str().ok()),
                    Some(expected)
                );
            }
            archive_events.push(event.clone());
        }
    }

    let expected = [
        (parent_id.as_str(), "archived"),
        (grandchild_id.as_str(), "archived"),
        (parent_id.as_str(), "unarchived"),
        (grandchild_id.as_str(), "unarchived"),
    ];
    assert_eq!(archive_events.len(), expected.len());
    for (event, (thread_id, action)) in archive_events.iter().zip(expected) {
        let occurred_at_ms = event["event_params"]["occurred_at_ms"]
            .as_u64()
            .expect("thread archive analytics must include its producer timestamp");
        assert_eq!(
            event,
            &json!({
                "event_type": "dex_thread_archive_event",
                "event_params": {
                    "thread_id": thread_id,
                    "action": action,
                    "occurred_at_ms": occurred_at_ms,
                },
            })
        );
    }

    Ok(())
}

#[tokio::test]
async fn thread_archive_succeeds_when_spawned_descendant_is_missing() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let dex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(dex_home.path())?;

    let parent_id = create_fake_rollout(
        dex_home.path(),
        "2025-01-01T00-00-00",
        "2025-01-01T00:00:00Z",
        "parent",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let parent_thread_id = ThreadId::from_string(&parent_id)?;
    let missing_child_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000901")?;

    let state_db = StateRuntime::init(
        dex_state::SqliteConfig::new_for_testing(dex_home.path().abs()),
        "mock_provider".into(),
    )
    .await?;
    state_db
        .mark_backfill_complete(/*last_watermark*/ None)
        .await?;
    state_db
        .upsert_thread_spawn_edge(
            parent_thread_id,
            missing_child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Closed,
        )
        .await?;

    let mut mcp = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;

    let _: ThreadArchiveResponse = mcp
        .request(|request_id| ClientRequest::ThreadArchive {
            request_id,
            params: ThreadArchiveParams {
                thread_id: parent_id.clone(),
            },
        })
        .await?;

    let archived_notification: ThreadArchivedNotification = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_notification("thread/archived"),
    )
    .await??;
    assert_eq!(archived_notification.thread_id, parent_id);

    assert!(
        find_thread_path_by_id_str(dex_home.path(), &parent_id, /*state_db_ctx*/ None)
            .await?
            .is_none(),
        "parent should be archived even when a descendant is missing"
    );
    assert!(
        find_archived_thread_path_by_id_str(
            dex_home.path(),
            &parent_id,
            /*state_db_ctx*/ None,
        )
        .await?
        .is_some(),
        "parent should be moved into archived sessions"
    );

    // Break descendant discovery without changing the stored parent.
    let pool = state_db
        .sqlite()
        .open_read_write_pool(&state_db.sqlite().state_db_path())
        .await?;
    sqlx::query("DROP TABLE thread_spawn_edges")
        .execute(&pool)
        .await?;
    unarchive(&mut mcp, &parent_id).await?;

    Ok(())
}

#[tokio::test]
async fn thread_archive_clears_stale_subscriptions_before_resume() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let dex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(dex_home.path())?;

    let mut primary = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .build_initialized()
        .await?;

    let ThreadStartResponse { thread, .. } = primary
        .start_thread(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;

    let _: TurnStartResponse = primary
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: thread.id.clone(),
                client_user_message_id: None,
                input: vec![UserInput::Text {
                    text: "materialize".to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        })
        .await?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        primary.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    primary.clear_message_buffer();

    let mut secondary = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .build_initialized()
        .await?;

    let _: ThreadArchiveResponse = primary
        .request(|request_id| ClientRequest::ThreadArchive {
            request_id,
            params: ThreadArchiveParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        primary.read_stream_until_notification_message("thread/archived"),
    )
    .await??;

    let _: ThreadUnarchiveResponse = primary
        .request(|request_id| ClientRequest::ThreadUnarchive {
            request_id,
            params: ThreadUnarchiveParams {
                thread_id: thread.id.clone(),
            },
        })
        .await?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        primary.read_stream_until_notification_message("thread/unarchived"),
    )
    .await??;
    primary.clear_message_buffer();

    let resume: ThreadResumeResponse = secondary
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: thread.id.clone(),
                ..Default::default()
            },
        })
        .await?;
    assert_eq!(resume.thread.status, ThreadStatus::Idle);
    primary.clear_message_buffer();
    secondary.clear_message_buffer();

    let _: TurnStartResponse = secondary
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: thread.id,
                client_user_message_id: None,
                input: vec![UserInput::Text {
                    text: "secondary turn".to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        })
        .await?;

    assert!(
        timeout(
            std::time::Duration::from_millis(250),
            primary.read_stream_until_notification_message("turn/started"),
        )
        .await
        .is_err()
    );

    timeout(
        DEFAULT_READ_TIMEOUT,
        secondary.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    Ok(())
}

fn assert_paths_match_on_disk(actual: &Path, expected: &Path) -> std::io::Result<()> {
    let actual = actual.canonicalize()?;
    let expected = expected.canonicalize()?;
    assert_eq!(actual, expected);
    Ok(())
}

async fn unarchive(mcp: &mut TestAppServer, thread_id: &str) -> Result<()> {
    let response: ThreadUnarchiveResponse = mcp
        .request(|request_id| ClientRequest::ThreadUnarchive {
            request_id,
            params: ThreadUnarchiveParams {
                thread_id: thread_id.to_string(),
            },
        })
        .await?;
    assert_eq!(response.thread.id, thread_id);
    Ok(())
}
