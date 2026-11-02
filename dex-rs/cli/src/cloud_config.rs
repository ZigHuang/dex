use anyhow::Context;
use anyhow::Result;
use dex_app_server::in_process::EmbeddedNetworkPolicy;
use dex_cloud_config::managed_config_bundle_loader_for_storage;
use dex_config::CloudConfigBundleLoader;
use dex_config::ConfigLoadOptions;
use dex_core::config::Config;
use dex_core::config::ConfigBuilder;
use dex_core::config::ConfigOverrides;
use dex_core::config::LoaderOverrides;
use dex_core::config::bootstrap_auth_config;
use dex_core::config::find_dex_home;
use dex_core::config::load_config_toml_with_layer_stack;
use dex_utils_absolute_path::AbsolutePathBuf;
use dex_utils_cli::CliConfigOverrides;

pub async fn load_config(
    config_overrides: &CliConfigOverrides,
    loader_overrides: LoaderOverrides,
) -> Result<Config> {
    let mut config = config_builder(
        config_overrides,
        loader_overrides,
        ConfigOverrides::default(),
    )
    .await?
    .build()
    .await
    .context("failed to load configuration")?;
    config.config_layer_stack.ensure_config_server_applied()?;
    EmbeddedNetworkPolicy::default().activate(&mut config);
    Ok(config)
}

pub async fn config_builder(
    config_overrides: &CliConfigOverrides,
    loader_overrides: LoaderOverrides,
    harness_overrides: ConfigOverrides,
) -> Result<ConfigBuilder> {
    let network_policy = EmbeddedNetworkPolicy::load(&loader_overrides).await;
    let cli_overrides = config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let dex_home = find_dex_home().context("failed to resolve DEX_HOME")?;
    let cwd = match harness_overrides.cwd.as_deref() {
        Some(cwd) => AbsolutePathBuf::relative_to_current_dir(cwd),
        None => AbsolutePathBuf::current_dir(),
    }
    .context("failed to resolve current directory")?;
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
    let cloud_config_bundle = managed_config_bundle_loader_for_storage(
        network_policy.bind_bootstrap_auth(
            bootstrap_auth_config(dex_home.as_path(), &bootstrap_config)
                .context("failed to resolve cloud configuration authentication")?,
        ),
        bootstrap_config.config_layer_stack.requirements_toml(),
        /*enable_dex_api_key_env*/ false,
    )
    .await
    .context("failed to initialize cloud configuration authentication")?;

    Ok(ConfigBuilder::default()
        .dex_home(dex_home.to_path_buf())
        .cli_overrides(cli_overrides)
        .loader_overrides(loader_overrides)
        .harness_overrides(harness_overrides)
        .cloud_config_bundle(cloud_config_bundle)
        .fallback_cwd(Some(cwd.to_path_buf())))
}
