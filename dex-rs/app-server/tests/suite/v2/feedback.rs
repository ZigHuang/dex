use anyhow::Result;
use app_test_support::TestAppServer;
use dex_app_server_protocol::RequestId;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

#[tokio::test]
async fn feedback_upload_returns_unsupported_error() -> Result<()> {
    let dex_home = TempDir::new()?;
    std::fs::write(
        dex_home.path().join("config.toml"),
        "[analytics]\nenabled = true\n[feedback]\nenabled = true\n",
    )?;
    let mut app_server = TestAppServer::builder()
        .with_dex_home(dex_home.path())
        .build_initialized()
        .await?;
    let request_id = app_server
        .send_raw_request(
            "feedback/upload",
            Some(json!({ "classification": "bug", "includeLogs": true })),
        )
        .await?;
    let error = timeout(
        Duration::from_secs(15),
        app_server.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(
        (error.error.code, error.error.message.as_str()),
        (-32600, "Feedback uploads are not supported by Dex.")
    );
    Ok(())
}
