# dex-otel

Dex retains local tracing and in-memory metrics, but does not export telemetry.
The OTLP exporters, Statsig defaults, and remote reporting transports have been
removed from the source and dependencies.

## Provider compatibility

`OtelProvider::try_new` always returns `Ok(None)`. Legacy `OtelSettings` and
`OtelExporter` values remain accepted for API/configuration compatibility, but
cannot enable reporting, even when an endpoint is explicitly configured.
Provider shutdown methods are no-ops.

## Local events and metrics

`SessionTelemetry` retains local tracing events. No exporting layer is installed
by Dex. Normal local log configuration still applies.

`MetricsClient` supports `MetricsConfig::in_memory` for local measurements and
tests. `shutdown()` flushes the in-memory exporter. Creating a client with
`MetricsConfig::otlp` returns `MetricsError::ExporterDisabled`; it does not make
network requests. The legacy global Statsig client getter always returns `None`.

## Trace context

W3C trace-context helpers remain available for local tracing and normal model
request propagation. They do not create a telemetry transport.

Removing reporting does not disable the application's normal model, MCP,
plugin, authentication, or update requests.
