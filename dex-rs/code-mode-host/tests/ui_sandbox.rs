#[test]
fn plugin_arguments_are_parsed_before_startup() -> Result<(), Box<dyn std::error::Error>> {
    let program = dex_utils_cargo_bin::cargo_bin("dex-code-mode-host")?;
    for arguments in [
        ["--help", "--dex-ui-plugin-process"],
        ["--dex-ui-plugin-process", "--help"],
    ] {
        let output = dex_utils_process::background_command(&program)
            .args(arguments)
            .output()?;
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)?.contains("Usage:"));
    }
    for arguments in [
        ["--unknown", "--dex-ui-plugin-process"],
        ["--dex-ui-plugin-process", "--unknown"],
        ["--listen=stdio", "--dex-ui-plugin-process"],
        ["--dex-ui-plugin-process", "--listen=stdio"],
    ] {
        let output = dex_utils_process::background_command(&program)
            .args(arguments)
            .output()?;
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr)?.contains("error:"));
    }
    Ok(())
}
