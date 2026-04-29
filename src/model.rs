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
pub struct LogEvent {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub service: String,
    pub message: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentMetricComparison {
    pub baseline_error_rate: f64,
    pub detected_error_rate: f64,
    pub baseline_latency_ms: f64,
    pub detected_latency_ms: f64,
    pub request_rate_at_detection: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentTimelineEvent {
    pub label: String,
    pub timestamp: DateTime<Utc>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionVerdict {
    pub deploy_id: String,
    pub environment: String,
    pub deploy_timestamp: DateTime<Utc>,
    pub detected_at: DateTime<Utc>,
    pub seconds_after_deploy: i64,
    pub error_rate_delta: f64,
    pub latency_delta_ms: f64,
    pub reason: String,
    pub top_error_signature: Option<String>,
    pub top_error_count: usize,
    pub top_error_is_new: bool,
    pub comparison: IncidentMetricComparison,
    pub timeline: Vec<IncidentTimelineEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub severity: String,
    pub summary: String,
    pub verdict: RegressionVerdict,
    pub alert_text: String,
    pub cached_explanation: Option<String>,
    pub cached_explanation_updated_at: Option<DateTime<Utc>>,
    pub status: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentListItem {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub severity: String,
    pub summary: String,
    pub deploy_id: String,
    pub environment: String,
    pub has_cached_explanation: bool,
    pub status: String,
    pub has_notes: bool,
}

impl Incident {
    pub fn list_item(&self) -> IncidentListItem {
        IncidentListItem {
            id: self.id.clone(),
            created_at: self.created_at,
            severity: self.severity.clone(),
            summary: self.summary.clone(),
            deploy_id: self.verdict.deploy_id.clone(),
            environment: self.verdict.environment.clone(),
            has_cached_explanation: self.cached_explanation.is_some(),
            status: self.status.clone(),
            has_notes: !self.notes.trim().is_empty(),
        }
    }
}
