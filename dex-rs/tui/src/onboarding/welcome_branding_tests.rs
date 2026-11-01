use super::*;

#[test]
fn welcome_renders_dex_branding_snapshot() {
    let is_logged_in = false;
    let animations_enabled = false;
    let widget = WelcomeWidget::new(
        is_logged_in,
        FrameRequester::test_dummy(),
        animations_enabled,
    );
    let area = Rect::new(0, 0, 80, 2);
    let mut buffer = Buffer::empty(area);
    (&widget).render_ref(area, &mut buffer);
    let rendered = (0..area.width)
        .map(|column| buffer[(column, 0)].symbol())
        .collect::<String>();

    insta::assert_snapshot!(rendered.trim_end(), @"  Welcome to Dex");
}
