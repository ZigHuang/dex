//! Windows sandbox configuration, runtime policy, and lifecycle diagnostics.

use crate::config::Config;
use crate::config::edit::ConfigEditsBuilder;
use dex_config::config_toml::ConfigToml;
use dex_config::types::WindowsSandboxModeToml;
use dex_features::Feature;
use dex_features::Features;
use dex_features::FeaturesToml;
use dex_login::default_client::originator;
use dex_otel::sanitize_metric_tag_value;
use dex_protocol::config_types::WindowsSandboxLevel;
use dex_protocol::models::PermissionProfile;
use dex_protocol::sandbox::effective_windows_sandbox_type;
use dex_sandboxing::SandboxType;
use dex_utils_absolute_path::AbsolutePathBuf;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

/// Records local MXC availability and the selected backend at startup.
pub fn log_windows_sandbox_startup(config: &Config) {
    #[cfg(windows)]
    {
        let sandbox_dir = dex_windows_sandbox::sandbox_dir(config.dex_home.as_path());
        let _ = std::fs::create_dir_all(&sandbox_dir);
        let available = dex_sandboxing::windows_mxc_available();
        dex_windows_sandbox::log_note(
            &format!("MXC availability: {available}"),
            Some(&sandbox_dir),
        );
        log_windows_sandbox_selection(
            config.dex_home.as_path(),
            config.effective_local_windows_sandbox_type(),
            WindowsSandboxLevel::from_config(config),
        );
    }
    #[cfg(not(windows))]
    let _ = config;
}

/// Records the local backend after an accepted selection change.
pub(crate) fn log_windows_sandbox_change(
    dex_home: &Path,
    sandbox_type: SandboxType,
    sandbox_level: WindowsSandboxLevel,
) {
    #[cfg(windows)]
    {
        let dex_home = dex_home.to_owned();
        tokio::task::spawn_blocking(move || {
            log_windows_sandbox_selection(&dex_home, sandbox_type, sandbox_level);
        });
    }
    #[cfg(not(windows))]
    let _ = (dex_home, sandbox_type, sandbox_level);
}

#[cfg(windows)]
fn log_windows_sandbox_selection(
    dex_home: &Path,
    sandbox_type: SandboxType,
    sandbox_level: WindowsSandboxLevel,
) {
    let backend = if sandbox_type == SandboxType::WindowsMxc {
        "mxc"
    } else {
        match sandbox_level {
            WindowsSandboxLevel::Disabled => "disabled",
            WindowsSandboxLevel::RestrictedToken => "unelevated",
            WindowsSandboxLevel::Elevated => "elevated",
        }
    };
    let sandbox_dir = dex_windows_sandbox::sandbox_dir(dex_home);
    let _ = std::fs::create_dir_all(&sandbox_dir);
    dex_windows_sandbox::log_note(
        &format!("Windows sandbox selection: {backend}"),
        Some(&sandbox_dir),
    );
}

/// Selects local binding policy from the sandbox and executor OS.
pub fn local_binding_policy_for_sandbox(
    sandbox_type: SandboxType,
    platform_os: Option<&str>,
) -> dex_network_proxy::LocalBindingPolicy {
    if sandbox_type == SandboxType::WindowsMxc && platform_os == Some("windows") {
        dex_network_proxy::LocalBindingPolicy::RequireTrue
    } else {
        dex_network_proxy::LocalBindingPolicy::DefaultFalse
    }
}

pub fn managed_proxy_routing_for_windows_sandbox(
    sandbox_type: SandboxType,
) -> dex_network_proxy::ManagedProxyRouting {
    if cfg!(windows) && sandbox_type == SandboxType::WindowsMxc {
        dex_network_proxy::ManagedProxyRouting::DedicatedListeners
    } else {
        dex_network_proxy::ManagedProxyRouting::SharedIngress
    }
}

/// Adapts the selected implementation for legacy checks that only understand setup levels.
/// Backend selection must continue to use [`SandboxType`] directly.
pub(crate) fn windows_sandbox_level_for_legacy_checks(
    sandbox_type: SandboxType,
    sandbox_level: WindowsSandboxLevel,
) -> WindowsSandboxLevel {
    if effective_windows_sandbox_type(sandbox_type, sandbox_level) == SandboxType::WindowsMxc {
        WindowsSandboxLevel::RestrictedToken
    } else {
        sandbox_level
    }
}

pub trait WindowsSandboxLevelExt {
    fn from_config(config: &Config) -> WindowsSandboxLevel;
    fn from_features(features: &Features) -> WindowsSandboxLevel;
}

impl WindowsSandboxLevelExt for WindowsSandboxLevel {
    fn from_config(config: &Config) -> WindowsSandboxLevel {
        match config.permissions.windows_sandbox_mode {
            Some(WindowsSandboxModeToml::Elevated) => WindowsSandboxLevel::Elevated,
            Some(WindowsSandboxModeToml::Unelevated) => WindowsSandboxLevel::RestrictedToken,
            Some(WindowsSandboxModeToml::Mxc) => WindowsSandboxLevel::Disabled,
            None => Self::from_features(&config.features),
        }
    }

    fn from_features(features: &Features) -> WindowsSandboxLevel {
        if features.enabled(Feature::WindowsSandboxElevated) {
            return WindowsSandboxLevel::Elevated;
        }
        if features.enabled(Feature::WindowsSandbox) {
            WindowsSandboxLevel::RestrictedToken
        } else {
            WindowsSandboxLevel::Disabled
        }
    }
}

pub fn resolve_windows_sandbox_mode(cfg: &ConfigToml) -> Option<WindowsSandboxModeToml> {
    cfg.windows
        .as_ref()
        .and_then(|windows| windows.sandbox)
        .or_else(|| legacy_windows_sandbox_mode(cfg.features.as_ref()))
}

pub fn legacy_windows_sandbox_mode(
    features: Option<&FeaturesToml>,
) -> Option<WindowsSandboxModeToml> {
    let entries = features.map(FeaturesToml::entries)?;
    legacy_windows_sandbox_mode_from_entries(&entries)
}

pub fn legacy_windows_sandbox_mode_from_entries(
    entries: &BTreeMap<String, bool>,
) -> Option<WindowsSandboxModeToml> {
    if entries
        .get(Feature::WindowsSandboxElevated.key())
        .copied()
        .unwrap_or(false)
    {
        return Some(WindowsSandboxModeToml::Elevated);
    }
    if entries
        .get(Feature::WindowsSandbox.key())
        .copied()
        .unwrap_or(false)
        || entries
            .get("enable_experimental_windows_sandbox")
            .copied()
            .unwrap_or(false)
    {
        Some(WindowsSandboxModeToml::Unelevated)
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
pub fn sandbox_setup_is_complete(dex_home: &Path) -> bool {
    dex_windows_sandbox::sandbox_setup_is_complete(dex_home)
}

#[cfg(not(target_os = "windows"))]
pub fn sandbox_setup_is_complete(_dex_home: &Path) -> bool {
    false
}

#[cfg(target_os = "windows")]
pub fn prepare_elevated_sandbox(
    permission_profile: &PermissionProfile,
    workspace_roots: &[AbsolutePathBuf],
    command_cwd: &Path,
    env_map: &HashMap<String, String>,
    dex_home: &Path,
) -> anyhow::Result<()> {
    if !sandbox_setup_is_complete(dex_home) {
        let permissions =
            dex_windows_sandbox::ResolvedWindowsSandboxPermissions::try_from_permission_profile_for_workspace_roots(
                permission_profile,
                workspace_roots,
            )?;
        dex_windows_sandbox::run_elevated_setup(dex_windows_sandbox::SandboxSetupRequest {
            permissions: &permissions,
            command_cwd,
            env_map,
            dex_home,
            proxy_enforced: false,
        })?;
    }
    dex_windows_sandbox::run_setup_refresh(
        permission_profile,
        workspace_roots,
        command_cwd,
        env_map,
        dex_home,
        /*proxy_enforced*/ false,
    )
}

#[cfg(any(target_os = "windows", test))]
fn provisioning_settings(
    network: Option<&crate::config::NetworkProxySpec>,
) -> std::io::Result<dex_windows_sandbox::WindowsSandboxProvisioningSettings> {
    let Some(network) = network.filter(|network| network.enabled()) else {
        return Ok(dex_windows_sandbox::WindowsSandboxProvisioningSettings::default());
    };
    Ok(dex_windows_sandbox::WindowsSandboxProvisioningSettings {
        proxy_ports: network.configured_proxy_ports()?,
        allow_local_binding: network.allow_local_binding(),
    })
}

#[cfg(target_os = "windows")]
pub fn run_elevated_provisioning_setup(
    dex_home: &Path,
    real_user: &str,
    network: Option<&crate::config::NetworkProxySpec>,
) -> anyhow::Result<()> {
    dex_windows_sandbox::run_elevated_provisioning_setup(
        dex_home,
        real_user,
        provisioning_settings(network)?,
    )
}

#[cfg(not(target_os = "windows"))]
pub fn prepare_elevated_sandbox(
    _permission_profile: &PermissionProfile,
    _workspace_roots: &[AbsolutePathBuf],
    _command_cwd: &Path,
    _env_map: &HashMap<String, String>,
    _dex_home: &Path,
) -> anyhow::Result<()> {
    anyhow::bail!("elevated Windows sandbox setup is only supported on Windows")
}

#[cfg(not(target_os = "windows"))]
pub fn run_elevated_provisioning_setup(
    _dex_home: &Path,
    _real_user: &str,
    _network: Option<&crate::config::NetworkProxySpec>,
) -> anyhow::Result<()> {
    anyhow::bail!("elevated Windows sandbox setup is only supported on Windows")
}

#[cfg(target_os = "windows")]
pub fn run_legacy_setup_preflight(
    permission_profile: &PermissionProfile,
    workspace_roots: &[AbsolutePathBuf],
    command_cwd: &Path,
    env_map: &HashMap<String, String>,
    dex_home: &Path,
) -> anyhow::Result<()> {
    dex_windows_sandbox::run_windows_sandbox_legacy_preflight(
        permission_profile,
        workspace_roots,
        dex_home,
        command_cwd,
        env_map,
    )
}

#[cfg(target_os = "windows")]
pub fn run_setup_refresh_with_extra_read_roots(
    permission_profile: &PermissionProfile,
    workspace_roots: &[AbsolutePathBuf],
    command_cwd: &Path,
    env_map: &HashMap<String, String>,
    dex_home: &Path,
    extra_read_roots: Vec<PathBuf>,
) -> anyhow::Result<()> {
    dex_windows_sandbox::run_setup_refresh_with_extra_read_roots(
        permission_profile,
        workspace_roots,
        command_cwd,
        env_map,
        dex_home,
        extra_read_roots,
        /*proxy_enforced*/ false,
    )
}

#[cfg(not(target_os = "windows"))]
pub fn run_legacy_setup_preflight(
    _permission_profile: &PermissionProfile,
    _workspace_roots: &[AbsolutePathBuf],
    _command_cwd: &Path,
    _env_map: &HashMap<String, String>,
    _dex_home: &Path,
) -> anyhow::Result<()> {
    anyhow::bail!("legacy Windows sandbox setup is only supported on Windows")
}

#[cfg(not(target_os = "windows"))]
pub fn run_setup_refresh_with_extra_read_roots(
    _permission_profile: &PermissionProfile,
    _workspace_roots: &[AbsolutePathBuf],
    _command_cwd: &Path,
    _env_map: &HashMap<String, String>,
    _dex_home: &Path,
    _extra_read_roots: Vec<PathBuf>,
) -> anyhow::Result<()> {
    anyhow::bail!("Windows sandbox read-root refresh is only supported on Windows")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsSandboxSetupMode {
    Elevated,
    Unelevated,
}

#[derive(Debug, Clone)]
pub struct WindowsSandboxSetupRequest {
    pub mode: WindowsSandboxSetupMode,
    pub permission_profile: PermissionProfile,
    pub workspace_roots: Vec<AbsolutePathBuf>,
    pub command_cwd: PathBuf,
    pub env_map: HashMap<String, String>,
    pub dex_home: PathBuf,
}

pub async fn run_windows_sandbox_setup(request: WindowsSandboxSetupRequest) -> anyhow::Result<()> {
    let start = Instant::now();
    let mode = request.mode;
    let originator_tag = sanitize_metric_tag_value(originator().value.as_str());
    let result = run_windows_sandbox_setup_and_persist(request).await;

    match result {
        Ok(()) => {
            emit_windows_sandbox_setup_success_metrics(
                mode,
                originator_tag.as_str(),
                start.elapsed(),
            );
            Ok(())
        }
        Err(err) => {
            emit_windows_sandbox_setup_failure_metrics(mode, start.elapsed(), &err);
            Err(err)
        }
    }
}

async fn run_windows_sandbox_setup_and_persist(
    request: WindowsSandboxSetupRequest,
) -> anyhow::Result<()> {
    let mode = request.mode;
    let permission_profile = request.permission_profile;
    let workspace_roots = request.workspace_roots;
    let command_cwd = request.command_cwd;
    let env_map = request.env_map;
    let dex_home = request.dex_home;
    let setup_dex_home = dex_home.clone();

    let setup_result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        match mode {
            WindowsSandboxSetupMode::Elevated => {
                prepare_elevated_sandbox(
                    &permission_profile,
                    workspace_roots.as_slice(),
                    command_cwd.as_path(),
                    &env_map,
                    setup_dex_home.as_path(),
                )?;
            }
            WindowsSandboxSetupMode::Unelevated => {
                run_legacy_setup_preflight(
                    &permission_profile,
                    workspace_roots.as_slice(),
                    command_cwd.as_path(),
                    &env_map,
                    setup_dex_home.as_path(),
                )?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|join_err| anyhow::anyhow!("windows sandbox setup task failed: {join_err}"))?;

    setup_result?;

    ConfigEditsBuilder::new(dex_home.as_path())
        .set_windows_sandbox_mode(windows_sandbox_setup_mode_tag(mode))
        .clear_legacy_windows_sandbox_keys()
        .apply()
        .await
        .map_err(|err| anyhow::anyhow!("failed to persist windows sandbox mode: {err}"))
}

fn emit_windows_sandbox_setup_success_metrics(
    mode: WindowsSandboxSetupMode,
    originator_tag: &str,
    duration: std::time::Duration,
) {
    let Some(metrics) = dex_otel::global() else {
        return;
    };
    let mode_tag = windows_sandbox_setup_mode_tag(mode);
    let _ = metrics.record_duration(
        "dex.windows_sandbox.setup_duration_ms",
        duration,
        &[
            ("result", "success"),
            ("originator", originator_tag),
            ("mode", mode_tag),
        ],
    );
    let _ = metrics.counter(
        "dex.windows_sandbox.setup_success",
        /*inc*/ 1,
        &[("originator", originator_tag), ("mode", mode_tag)],
    );
}

/// Records setup failures, including service attempts that fail before the helper path.
pub fn emit_windows_sandbox_setup_failure_metrics(
    mode: WindowsSandboxSetupMode,
    duration: std::time::Duration,
    _err: &anyhow::Error,
) {
    let Some(metrics) = dex_otel::global() else {
        return;
    };
    let originator_tag = sanitize_metric_tag_value(originator().value.as_str());
    let originator_tag = originator_tag.as_str();
    let mode_tag = windows_sandbox_setup_mode_tag(mode);
    let _ = metrics.record_duration(
        "dex.windows_sandbox.setup_duration_ms",
        duration,
        &[
            ("result", "failure"),
            ("originator", originator_tag),
            ("mode", mode_tag),
        ],
    );
    let _ = metrics.counter(
        "dex.windows_sandbox.setup_failure",
        /*inc*/ 1,
        &[("originator", originator_tag), ("mode", mode_tag)],
    );

    if matches!(mode, WindowsSandboxSetupMode::Elevated) {
        #[cfg(target_os = "windows")]
        {
            let mut failure_tags: Vec<(&str, &str)> = vec![("originator", originator_tag)];
            let mut code_tag: Option<String> = None;
            let mut message_tag: Option<String> = None;
            if let Some(failure) = dex_windows_sandbox::extract_setup_failure(_err) {
                code_tag = Some(failure.code.as_str().to_string());
                message_tag = Some(dex_windows_sandbox::sanitize_setup_metric_tag_value(
                    &failure.message,
                ));
            }
            if let Some(code) = code_tag.as_deref() {
                failure_tags.push(("code", code));
            }
            if let Some(message) = message_tag.as_deref() {
                failure_tags.push(("message", message));
            }
            let metric_name =
                if dex_windows_sandbox::extract_setup_failure(_err).is_some_and(|failure| {
                    matches!(
                        failure.code,
                        dex_windows_sandbox::SetupErrorCode::OrchestratorHelperLaunchCanceled
                    )
                }) {
                    "dex.windows_sandbox.elevated_setup_canceled"
                } else {
                    "dex.windows_sandbox.elevated_setup_failure"
                };
            let _ = metrics.counter(metric_name, /*inc*/ 1, &failure_tags);
        }
    } else {
        let _ = metrics.counter(
            "dex.windows_sandbox.legacy_setup_preflight_failed",
            /*inc*/ 1,
            &[("originator", originator_tag)],
        );
    }
}

fn windows_sandbox_setup_mode_tag(mode: WindowsSandboxSetupMode) -> &'static str {
    match mode {
        WindowsSandboxSetupMode::Elevated => "elevated",
        WindowsSandboxSetupMode::Unelevated => "unelevated",
    }
}

#[cfg(test)]
#[path = "windows_sandbox_tests.rs"]
mod tests;
