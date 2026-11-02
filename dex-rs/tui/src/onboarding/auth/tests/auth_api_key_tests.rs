use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn api_key_setup_accepts_pasted_credentials() {
    let (mut widget, _dex_home) = widget_forced_chatgpt().await;
    widget.auth_config.forced_login_method = None;
    widget.highlighted_mode = SignInOption::ApiKey;

    widget.start_api_key_entry();
    assert!(widget.is_text_entry_active());
    *widget.sign_in_state.write().unwrap() = SignInState::ApiKeyEntry(ApiKeyInputState {
        value: String::new(),
        prepopulated_from_env: false,
    });
    widget.handle_paste("sk-provider-test".to_string());

    let state = widget.sign_in_state.read().unwrap();
    let SignInState::ApiKeyEntry(input) = &*state else {
        panic!("API key entry should remain active");
    };
    assert_eq!(
        (input.value.as_str(), input.prepopulated_from_env),
        ("sk-provider-test", false)
    );
    assert_eq!(widget.error_message(), None);
}

#[tokio::test]
async fn workspace_api_access_guidance_renders_snapshot() {
    let (widget, _dex_home) = widget_forced_chatgpt().await;
    let area = Rect::new(0, 0, 80, 8);
    let mut buffer = Buffer::empty(area);
    widget.render_pick_mode(area, &mut buffer);
    let rendered = (0..area.height)
        .map(|row| {
            (0..area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered.trim_end(), @r###"
      Configure API access

      API key login is disabled by this workspace.
      Ask your administrator to enable API key access.
    "###);
}
