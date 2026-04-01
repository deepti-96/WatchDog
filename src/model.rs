use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub timestamp: DateTime<Utc>,
    pub error_rate: f64,
    pub p95_latency_ms: f64,
    pub request_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployEvent {
    pub timestamp: DateTime<Utc>,
    pub deploy_id: String,
    pub environment: String,
}

#[derive(Debug, Clone)]
pub struct BaselineSnapshot {
    pub error_rate_mean: f64,
    pub p95_latency_mean: f64,
    pub sample_count: usize,
}

#[derive(Debug, Clone)]
pub struct RegressionVerdict {
    pub deploy_id: String,
    pub detected_at: DateTime<Utc>,
    pub seconds_after_deploy: i64,
    pub error_rate_delta: f64,
    pub latency_delta_ms: f64,
    pub reason: String,
}
