//! Logging, tracing and metrics.
//!
//! Logs go to stderr as text or JSON. When `OTEL_EXPORTER_OTLP_ENDPOINT` is
//! set, spans and metrics are also exported over OTLP/HTTP; the exporter reads
//! the standard `OTEL_*` variables. Logs never contain document content or
//! file names (only [`name_hash`] prefixes).

use std::collections::BTreeMap;

use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram},
    propagation::{Extractor, Injector},
    trace::TracerProvider as _,
};
use opentelemetry_sdk::{
    Resource, metrics::SdkMeterProvider, propagation::TraceContextPropagator,
    trace::SdkTracerProvider,
};
use sha2::{Digest, Sha256};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::{LogFormat, TelemetryConfig};

/// Handles that flush exporters on shutdown.
#[derive(Default)]
pub struct Telemetry {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
}

impl Telemetry {
    /// Flushes and stops the exporters.
    pub fn shutdown(self) {
        if let Some(tracer) = self.tracer
            && let Err(err) = tracer.shutdown()
        {
            tracing::warn!(error = %err, "trace exporter shutdown failed");
        }
        if let Some(meter) = self.meter
            && let Err(err) = meter.shutdown()
        {
            tracing::warn!(error = %err, "metric exporter shutdown failed");
        }
    }
}

/// Installs the global subscriber, and OTLP exporters when configured.
pub fn init(config: &TelemetryConfig, default_service: &str) -> Result<Telemetry, String> {
    global::set_text_map_propagator(TraceContextPropagator::new());
    let filter = EnvFilter::try_new(&config.log_filter).map_err(|e| e.to_string())?;
    let service = config
        .service_name
        .clone()
        .unwrap_or_else(|| default_service.to_owned());

    let mut telemetry = Telemetry::default();
    let otel_layer = if config.otlp_endpoint.is_some() {
        let resource = Resource::builder().with_service_name(service).build();
        let spans = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()
            .map_err(|e| e.to_string())?;
        let tracer_provider = SdkTracerProvider::builder()
            .with_batch_exporter(spans)
            .with_resource(resource.clone())
            .build();
        let metrics = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .build()
            .map_err(|e| e.to_string())?;
        let meter_provider = SdkMeterProvider::builder()
            .with_periodic_exporter(metrics)
            .with_resource(resource)
            .build();
        global::set_meter_provider(meter_provider.clone());
        global::set_tracer_provider(tracer_provider.clone());
        let tracer = tracer_provider.tracer("ariad-server");
        telemetry.tracer = Some(tracer_provider);
        telemetry.meter = Some(meter_provider);
        Some(tracing_opentelemetry::layer().with_tracer(tracer))
    } else {
        None
    };

    let registry = tracing_subscriber::registry().with(filter).with(otel_layer);
    let result = match config.log_format {
        LogFormat::Text => registry
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .try_init(),
        LogFormat::Json => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_writer(std::io::stderr),
            )
            .try_init(),
    };
    result.map_err(|e| e.to_string())?;
    Ok(telemetry)
}

/// A short, non-reversible tag for a file name, safe for logs.
#[must_use]
pub fn name_hash(name: &str) -> String {
    let digest = Sha256::digest(name.as_bytes());
    hex::encode(&digest[..6])
}

struct MapInjector<'a>(&'a mut BTreeMap<String, String>);

impl Injector for MapInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }
}

struct MapExtractor<'a>(&'a BTreeMap<String, String>);

impl Extractor for MapExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }
}

/// The current span's trace context, for a queue message's headers.
#[must_use]
pub fn inject_current() -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    let context = tracing::Span::current().context();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut MapInjector(&mut headers));
    });
    headers
}

/// Makes `span` a child of the trace carried in a message's headers.
pub fn set_parent_from(span: &tracing::Span, headers: &BTreeMap<String, String>) {
    let parent =
        global::get_text_map_propagator(|propagator| propagator.extract(&MapExtractor(headers)));
    let _ = span.set_parent(parent);
}

/// The metric instruments named in ARCHITECTURE §13.
#[derive(Clone)]
pub struct Metrics {
    conversion_duration: Histogram<f64>,
    conversion_success: Histogram<f64>,
    queue_wait: Histogram<f64>,
    pages_processed: Counter<u64>,
    engine_peak_memory: Histogram<u64>,
    engine_crash: Counter<u64>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    #[must_use]
    pub fn new() -> Self {
        let meter = global::meter("ariad-server");
        Self {
            conversion_duration: meter
                .f64_histogram("conversion_duration_seconds")
                .with_unit("s")
                .with_description("Wall time of one conversion job attempt")
                .build(),
            conversion_success: meter
                .f64_histogram("conversion_success_ratio")
                .with_description(
                    "1 for a succeeded job and 0 for a failed one; the mean is the success ratio",
                )
                .build(),
            queue_wait: meter
                .f64_histogram("queue_wait_seconds")
                .with_unit("s")
                .with_description("Time between enqueue and a worker picking the job up")
                .build(),
            pages_processed: meter
                .u64_counter("pages_processed_total")
                .with_description("Pages converted, where the route reports pages")
                .build(),
            engine_peak_memory: meter
                .u64_histogram("engine_peak_memory_bytes")
                .with_unit("By")
                .with_description("Peak resident memory of an engine process tree")
                .build(),
            engine_crash: meter
                .u64_counter("engine_crash_total")
                .with_description("Engine processes that crashed or broke the protocol")
                .build(),
        }
    }

    pub fn conversion(&self, route: &str, profile: &str, seconds: f64, success: bool) {
        let labels = [
            KeyValue::new("route", route.to_owned()),
            KeyValue::new("profile", profile.to_owned()),
        ];
        self.conversion_duration.record(seconds, &labels);
        self.conversion_success
            .record(if success { 1.0 } else { 0.0 }, &labels);
    }

    pub fn queue_wait(&self, pack: &str, seconds: f64) {
        self.queue_wait
            .record(seconds.max(0.0), &[KeyValue::new("pack", pack.to_owned())]);
    }

    pub fn pages(&self, pages: u64) {
        if pages > 0 {
            self.pages_processed.add(pages, &[]);
        }
    }

    pub fn peak_memory(&self, bytes: u64) {
        self.engine_peak_memory.record(bytes, &[]);
    }

    pub fn engine_crash(&self, engine: &str) {
        self.engine_crash
            .add(1, &[KeyValue::new("engine", engine.to_owned())]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_hash_is_short_and_stable() {
        let a = name_hash("report.docx");
        assert_eq!(a.len(), 12);
        assert_eq!(a, name_hash("report.docx"));
        assert_ne!(a, name_hash("report2.docx"));
        assert!(!a.contains("report"));
    }

    #[test]
    fn trace_context_round_trips_through_headers() {
        global::set_text_map_propagator(TraceContextPropagator::new());
        let mut headers = BTreeMap::new();
        headers.insert(
            "traceparent".to_owned(),
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        );
        let context = global::get_text_map_propagator(|p| p.extract(&MapExtractor(&headers)));
        let mut out = BTreeMap::new();
        global::get_text_map_propagator(|p| p.inject_context(&context, &mut MapInjector(&mut out)));
        assert_eq!(out.get("traceparent"), headers.get("traceparent"));
    }
}
