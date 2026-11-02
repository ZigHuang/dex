use anyhow::Result;
use core_test_support::TempDirExt;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_dex::local_requests;
use core_test_support::test_dex::test_dex;
use core_test_support::wait_for_event;
use dex_core::config::Constrained;
use dex_protocol::config_types::CollaborationMode;
use dex_protocol::config_types::ModeKind;
use dex_protocol::config_types::Settings;
use dex_protocol::protocol::AskForApproval;
use dex_protocol::protocol::EventMsg;
use dex_protocol::protocol::Op;
use tempfile::TempDir;

fn collab_mode_with_instructions(instructions: Option<&str>) -> CollaborationMode {
    CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5.4".to_string(),
            reasoning_effort: None,
            developer_instructions: instructions.map(str::to_string),
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_settings_update_without_user_turn_does_not_record_permissions_update() -> Result<()>
{
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let mut builder = test_dex().with_config(|config| {
        config.permissions.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    });
    let test = builder.build(&server).await?;

    core_test_support::submit_thread_settings(
        &test.dex,
        dex_protocol::protocol::ThreadSettingsOverrides {
            approval_policy: Some(AskForApproval::Never),
            ..Default::default()
        },
    )
    .await?;

    test.dex.submit(Op::Shutdown).await?;
    wait_for_event(&test.dex, |ev| matches!(ev, EventMsg::ShutdownComplete)).await;

    let rollout_path = test.dex.rollout_path().expect("rollout path");
    assert!(
        !rollout_path.exists(),
        "did not expect a rollout before a new user turn"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_settings_update_without_user_turn_does_not_record_environment_update() -> Result<()>
{
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let test = test_dex().build(&server).await?;
    let new_cwd = TempDir::new()?;

    core_test_support::submit_thread_settings(
        &test.dex,
        dex_protocol::protocol::ThreadSettingsOverrides {
            environments: Some(local_requests(new_cwd.abs())),
            ..Default::default()
        },
    )
    .await?;

    test.dex.submit(Op::Shutdown).await?;
    wait_for_event(&test.dex, |ev| matches!(ev, EventMsg::ShutdownComplete)).await;

    let rollout_path = test.dex.rollout_path().expect("rollout path");
    assert!(
        !rollout_path.exists(),
        "did not expect a rollout before a new user turn"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_settings_update_without_user_turn_does_not_record_collaboration_update()
-> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let test = test_dex().build(&server).await?;
    let collab_text = "override collaboration instructions";
    let collaboration_mode = collab_mode_with_instructions(Some(collab_text));

    core_test_support::submit_thread_settings(
        &test.dex,
        dex_protocol::protocol::ThreadSettingsOverrides {
            collaboration_mode: Some(collaboration_mode),
            ..Default::default()
        },
    )
    .await?;

    test.dex.submit(Op::Shutdown).await?;
    wait_for_event(&test.dex, |ev| matches!(ev, EventMsg::ShutdownComplete)).await;

    let rollout_path = test.dex.rollout_path().expect("rollout path");
    assert!(
        !rollout_path.exists(),
        "did not expect a rollout before a new user turn"
    );

    Ok(())
}
