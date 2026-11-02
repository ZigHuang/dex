use anyhow::Result;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_dex::test_dex;
use core_test_support::wait_for_event;
use dex_core::TurnInputRequest;
use dex_protocol::protocol::DexErrorInfo;
use dex_protocol::protocol::EventMsg;
use dex_protocol::user_input::UserInput;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test_case::test_case("insufficient_quota"; "quota")]
#[test_case::test_case("credit_balance_exhausted"; "credit_balance")]
#[test_case::test_case("organization_spend_limit_exceeded"; "organization_spend_limit")]
#[test_case::test_case("project_spend_limit_exceeded"; "project_spend_limit")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quota_exceeded_emits_single_error_event(code: &str) -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let mut builder = test_dex();

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            json!({
                "type": "response.failed",
                "response": {
                    "id": "resp-1",
                    "error": {
                        "code": code,
                        "message": "You exceeded your current quota, please check your plan and billing details."
                    }
                }
            }),
        ]),
    )
    .await;

    let test = builder.build_with_auto_env(&server).await?;

    test.dex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "quota?".into(),
            text_elements: Vec::new(),
        }]))
        .await?;

    let mut error_events = 0;

    loop {
        let event = wait_for_event(&test.dex, |_| true).await;

        match event {
            EventMsg::Error(err) => {
                error_events += 1;
                assert_eq!(
                    err.message,
                    "Quota exceeded. Check your plan and billing details."
                );
                assert_eq!(err.dex_error_info, Some(DexErrorInfo::UsageLimitExceeded));
            }
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }

    assert_eq!(error_events, 1, "expected exactly one Dex:Error event");
    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.url.path().ends_with("/responses"))
            .count(),
        1
    );

    Ok(())
}
