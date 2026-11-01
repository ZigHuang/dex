mod common;

use common::exec_server::exec_server;
use dex_config::CONFIG_TOML_FILE;
use dex_config::ConfigLayerSource;
use dex_config::format_config_layer_source;
use dex_config::loader::project_trust_key;
use dex_exec_server::Environment;
use dex_exec_server::EnvironmentConfigLayer;
use dex_exec_server::EnvironmentConfigLayerStack;
use dex_exec_server::EnvironmentConfigReadParams;
use dex_exec_server::EnvironmentConfigReadResponse;
use dex_exec_server::ExecServerError;
use dex_utils_absolute_path::AbsolutePathBuf;
use dex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_environment_reads_projected_executor_config() -> anyhow::Result<()> {
    let mut server = exec_server().await?;
    let dex_home = AbsolutePathBuf::from_absolute_path(std::fs::canonicalize(server.dex_home())?)?;
    let config_file = dex_home.join(CONFIG_TOML_FILE);
    let project = dex_home.join("project");
    let dot_dex = project.join(".dex");
    tokio::fs::create_dir_all(dot_dex.as_path()).await?;
    tokio::fs::write(project.join(".project-root").as_path(), "").await?;
    let project_key = toml::Value::String(project_trust_key(project.as_path())).to_string();
    tokio::fs::write(
        &config_file,
        format!(
            "project_root_markers = [\".project-root\"]\n[projects.{project_key}]\ntrust_level = \"trusted\""
        ),
    )
    .await?;
    tokio::fs::write(
        dot_dex.join(CONFIG_TOML_FILE).as_path(),
        r#"
[future_environment]
relative_path = "./executor-relative"
unselected = "do not return"
"#,
    )
    .await?;

    let environment = Environment::create_for_tests(Some(server.websocket_url().to_string()))?;
    let environment_info = environment.info().await?;
    assert!(environment_info.capabilities.environment_config_read);

    let response = environment
        .read_environment_config(EnvironmentConfigReadParams {
            cwd: PathUri::from_abs_path(&project),
            config_paths: vec![vec![
                "future_environment".to_string(),
                "relative_path".to_string(),
            ]],
            requirements_paths: Vec::new(),
        })
        .await?;

    let projected_toml = toml::toml! {
        [future_environment]
        relative_path = "./executor-relative"
    };
    assert_eq!(
        response,
        EnvironmentConfigReadResponse {
            user_home_dir: dirs::home_dir()
                .and_then(|home_dir| PathUri::from_host_native_path(home_dir).ok()),
            dex_home_dir: PathUri::from_abs_path(&dex_home),
            hostname: dex_config::host_name(),
            config: EnvironmentConfigLayerStack {
                layers: vec![EnvironmentConfigLayer {
                    source: format_config_layer_source(
                        &ConfigLayerSource::Project {
                            dot_dex_folder: dot_dex.clone(),
                        },
                        CONFIG_TOML_FILE,
                    ),
                    base_dir: PathUri::from_abs_path(&dot_dex),
                    toml: toml::to_string(&projected_toml)?,
                }],
                cloud_insertion_index: 0,
            },
            requirements: EnvironmentConfigLayerStack {
                layers: Vec::new(),
                cloud_insertion_index: 0,
            },
        }
    );

    server.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn environment_config_read_rejects_empty_selectors() -> anyhow::Result<()> {
    let mut server = exec_server().await?;
    let dex_home = AbsolutePathBuf::from_absolute_path(std::fs::canonicalize(server.dex_home())?)?;
    let environment = Environment::create_for_tests(Some(server.websocket_url().to_string()))?;

    for (config_paths, expected_message) in [
        (
            Vec::new(),
            "at least one config or requirements path is required",
        ),
        (
            vec![Vec::new()],
            "TOML paths must contain at least one key segment",
        ),
    ] {
        let error = environment
            .read_environment_config(EnvironmentConfigReadParams {
                cwd: PathUri::from_abs_path(&dex_home),
                config_paths,
                requirements_paths: Vec::new(),
            })
            .await
            .expect_err("invalid selectors should fail");
        assert!(
            matches!(
                error,
                ExecServerError::Server { code: -32602, ref message }
                    if message == expected_message
            ),
            "unexpected error: {error:?}"
        );
    }

    server.shutdown().await?;
    Ok(())
}
