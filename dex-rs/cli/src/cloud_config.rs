use anyhow::Context;
use anyhow::Result;
use dex_cloud_config::cloud_config_bundle_loader_for_storage;
use dex_config::CloudConfigBundleLoader;
use dex_config::ConfigLoadOptions;
use dex_core::config::Config;
use dex_core::config::ConfigBuilder;
use dex_core::config::LoaderOverrides;
use dex_core::config::bootstrap_auth_config;
use dex_core::config::find_dex_home;
use dex_core::config::load_config_toml_with_layer_stack;
use dex_utils_absolute_path::AbsolutePathBuf;
use dex_utils_cli::CliConfigOverrides;

pub(crate) async fn load_config(
    config_overrides: &CliConfigOverrides,
    loader_overrides: LoaderOverrides,
) -> Result<Config> {
    let cli_overrides = config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let dex_home = find_dex_home().context("failed to resolve DEX_HOME")?;
    let cwd = AbsolutePathBuf::current_dir().context("failed to resolve current directory")?;
    let bootstrap_config = load_config_toml_with_layer_stack(
        dex_home.as_path(),
        Some(&cwd),
        cli_overrides.clone(),
        ConfigLoadOptions {
            loader_overrides: loader_overrides.clone(),
            strict_config: false,
            cloud_config_bundle: CloudConfigBundleLoader::default(),
        },
    )
    .await
    .context("failed to load bootstrap configuration")?;
    let cloud_config_bundle = cloud_config_bundle_loader_for_storage(
        bootstrap_auth_config(dex_home.as_path(), &bootstrap_config)
            .context("failed to resolve cloud configuration authentication")?,
        /*enable_dex_api_key_env*/ false,
    )
    .await
    .context("failed to initialize cloud configuration authentication")?;

    ConfigBuilder::default()
        .dex_home(dex_home.to_path_buf())
        .cli_overrides(cli_overrides)
        .loader_overrides(loader_overrides)
        .cloud_config_bundle(cloud_config_bundle)
        .build()
        .await
        .context("failed to load configuration")
}
