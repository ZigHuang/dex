use anyhow::Result;
use predicates::str::contains;
use std::path::Path;
use tempfile::TempDir;

fn dex_command(dex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(dex_utils_cargo_bin::cargo_bin("dex")?);
    cmd.env("DEX_HOME", dex_home);
    Ok(cmd)
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn update_does_not_start_interactive_prompt() -> Result<()> {
    let dex_home = TempDir::new()?;

    dex_command(dex_home.path())?
        .arg("update")
        .assert()
        .failure()
        .stderr(contains("`dex update` is not available in debug builds"));

    Ok(())
}
