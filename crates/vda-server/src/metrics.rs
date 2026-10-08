//! Bounded-label Prometheus recorder installation.
/// Install the process-global metrics recorder and return a render handle.
pub fn install() -> anyhow::Result<metrics_exporter_prometheus::PrometheusHandle> {
    Ok(metrics_exporter_prometheus::PrometheusBuilder::new().install_recorder()?)
}
