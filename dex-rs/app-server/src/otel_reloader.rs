use crate::OTEL_SERVICE_NAME;
use crate::config_manager::ConfigManager;
use dex_login::AuthManager;
use dex_otel::OtelProvider;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::reload;

type OtelExportLayer<S> = Option<Box<dyn Layer<S> + Send + Sync + 'static>>;
type OtelReloadLayers<S> = (
    Vec<Box<dyn Layer<S> + Send + Sync + 'static>>,
    reload::Handle<OtelExportLayer<S>, S>,
);

pub(crate) fn layers<S>(provider: Option<&OtelProvider>) -> OtelReloadLayers<S>
where
    S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync + 'static,
{
    let logger_export_layer: OtelExportLayer<S> = provider
        .and_then(OtelProvider::logger_export_layer)
        .map(Layer::boxed);
    let (logger_layer, logger_handle) = reload::Layer::new(logger_export_layer);

    let layers: Vec<Box<dyn Layer<S> + Send + Sync + 'static>> = vec![
        logger_layer
            .with_filter(tracing_subscriber::filter::filter_fn(
                OtelProvider::log_export_filter,
            ))
            .boxed(),
        // The subscriber graph is fixed after startup. Keep the global tracer
        // bridge installed even when tracing starts disabled so an account
        // switch can install a trace exporter later.
        OtelProvider::reloadable_tracing_layer(OTEL_SERVICE_NAME).boxed(),
    ];
    (layers, logger_handle)
}

pub(crate) fn spawn<S>(
    _provider: Option<OtelProvider>,
    _logger_reload_handle: reload::Handle<OtelExportLayer<S>, S>,
    _config_manager: ConfigManager,
    _auth_manager: Arc<AuthManager>,
    _default_analytics_enabled: bool,
    shutdown_token: CancellationToken,
) -> JoinHandle<()>
where
    S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync + 'static,
{
    tokio::spawn(async move {
        shutdown_token.cancelled().await;
    })
}
