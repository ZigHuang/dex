#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    dex_windows_sandbox::setup_helper_main()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("dex-windows-sandbox-setup is Windows-only");
}
