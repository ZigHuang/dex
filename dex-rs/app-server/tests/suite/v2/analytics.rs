use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use dex_config::types::AuthCredentialsStoreMode;
use serde_json::Value;
use std::path::Path;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

pub(crate) async fn mount_analytics_capture(server: &MockServer, dex_home: &Path) -> Result<()> {
    Mock::given(method("POST"))
        .and(path("/dex/analytics-events/events"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;

    write_chatgpt_auth(
        dex_home,
        ChatGptAuthFixture::new("chatgpt-token")
            .account_id("account-123")
            .chatgpt_user_id("user-123")
            .chatgpt_account_id("account-123"),
        AuthCredentialsStoreMode::File,
    )?;

    app_test_support::mount_workspace_routing(server).await;
    Ok(())
}

pub(crate) async fn captured_analytics_events(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|request| request.url.path() == "/dex/analytics-events/events")
        .flat_map(|request| {
            let payload: Value = serde_json::from_slice(&request.body).expect("analytics payload");
            payload["events"]
                .as_array()
                .expect("analytics events")
                .clone()
        })
        .collect()
}
