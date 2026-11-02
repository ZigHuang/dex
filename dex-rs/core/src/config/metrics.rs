//! Metrics derived from loaded configuration at session start.

use super::Config;
use dex_features::FEATURES;
use dex_features::Stage;
use dex_otel::SessionTelemetry;

pub(crate) fn emit_session_start_metrics(config: &Config, telemetry: &SessionTelemetry) {
    for feature in FEATURES {
        if matches!(feature.stage, Stage::Removed) {
            continue;
        }
        if config.features.enabled(feature.id) != feature.default_enabled {
            telemetry.counter(
                "dex.feature.state",
                /*inc*/ 1,
                &[
                    ("feature", feature.key),
                    ("value", &config.features.enabled(feature.id).to_string()),
                ],
            );
        }
    }
    #[cfg(windows)]
    crate::windows_system_config::emit_namespace_squatting_probe();
}
