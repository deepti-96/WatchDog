use crate::detector::DetectorSettings;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs::File;
use std::path::{Path, PathBuf};

pub const DEFAULT_BASELINE_CAPACITY: usize = 120;
pub const DEFAULT_MONITORING_WINDOW_SECS: u64 = 300;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WatchdogConfig {
    pub baseline_capacity: Option<usize>,
    pub monitoring_window_secs: Option<u64>,
    pub log_file: Option<PathBuf>,
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub detector: DetectorConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DetectorConfig {
    pub error_threshold: Option<f64>,
    pub error_drift: Option<f64>,
    pub latency_threshold: Option<f64>,
    pub latency_drift: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RunSettings {
    pub baseline_capacity: usize,
    pub monitoring_window_secs: u64,
    pub log_file: Option<PathBuf>,
    pub webhook_url: Option<String>,
    pub detector: DetectorSettings,
}

impl WatchdogConfig {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };

        let file = File::open(path)
            .with_context(|| format!("failed to open config {}", path.display()))?;
        serde_json::from_reader(file)
            .with_context(|| format!("failed to parse JSON config {}", path.display()))
    }

    pub fn resolve_run_settings(
        self,
        cli_log_file: Option<PathBuf>,
        cli_monitoring_window_secs: Option<u64>,
        cli_webhook_url: Option<String>,
    ) -> RunSettings {
        let detector_defaults = DetectorSettings::default();
        RunSettings {
            baseline_capacity: self.baseline_capacity.unwrap_or(DEFAULT_BASELINE_CAPACITY),
            monitoring_window_secs: cli_monitoring_window_secs
                .or(self.monitoring_window_secs)
                .unwrap_or(DEFAULT_MONITORING_WINDOW_SECS),
            log_file: cli_log_file.or(self.log_file),
            webhook_url: cli_webhook_url.or(self.webhook_url),
            detector: DetectorSettings {
                error_threshold: self
                    .detector
                    .error_threshold
                    .unwrap_or(detector_defaults.error_threshold),
                error_drift: self.detector.error_drift.unwrap_or(detector_defaults.error_drift),
                latency_threshold: self
                    .detector
                    .latency_threshold
                    .unwrap_or(detector_defaults.latency_threshold),
                latency_drift: self
                    .detector
                    .latency_drift
                    .unwrap_or(detector_defaults.latency_drift),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn config_resolves_defaults_and_cli_overrides() {
        let config = WatchdogConfig {
            monitoring_window_secs: Some(120),
            webhook_url: Some("https://hooks.example.test/from-config".to_string()),
            detector: DetectorConfig {
                error_threshold: Some(0.12),
                ..DetectorConfig::default()
            },
            ..WatchdogConfig::default()
        };

        let settings = config.resolve_run_settings(
            Some(PathBuf::from("custom.log")),
            Some(45),
            Some("https://hooks.example.test/from-cli".to_string()),
        );

        assert_eq!(settings.baseline_capacity, DEFAULT_BASELINE_CAPACITY);
        assert_eq!(settings.monitoring_window_secs, 45);
        assert_eq!(settings.log_file, Some(PathBuf::from("custom.log")));
        assert_eq!(
            settings.webhook_url,
            Some("https://hooks.example.test/from-cli".to_string())
        );
        assert_eq!(settings.detector.error_threshold, 0.12);
        assert_eq!(
            settings.detector.latency_threshold,
            DetectorSettings::default().latency_threshold
        );
    }

    #[test]
    fn config_loads_json_file() {
        let path = std::env::temp_dir().join(format!(
            "watchdog-config-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"{
              "baseline_capacity": 32,
              "monitoring_window_secs": 90,
              "detector": {
                "latency_threshold": 75.0
              }
            }"#,
        )
        .expect("write config");

        let config = WatchdogConfig::load(Some(&path)).expect("load config");
        let settings = config.resolve_run_settings(None, None, None);

        assert_eq!(settings.baseline_capacity, 32);
        assert_eq!(settings.monitoring_window_secs, 90);
        assert_eq!(settings.detector.latency_threshold, 75.0);

        let _ = fs::remove_file(path);
    }
}
