use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const INCIDENT_STATUS_OPEN: &str = "open";
pub const INCIDENT_STATUS_RESOLVED: &str = "resolved";

pub fn normalize_incident_status(status: &str) -> Option<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        INCIDENT_STATUS_OPEN => Some(INCIDENT_STATUS_OPEN),
        INCIDENT_STATUS_RESOLVED => Some(INCIDENT_STATUS_RESOLVED),
        _ => None,
    }
}

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
    #[serde(default)]
    pub cached_explanation: Option<String>,
    #[serde(default)]
    pub cached_explanation_updated_at: Option<DateTime<Utc>>,
    #[serde(default = "default_incident_status")]
    pub status: String,
    #[serde(default)]
    pub notes: String,
}

fn default_incident_status() -> String {
    INCIDENT_STATUS_OPEN.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incident_status_normalizes_expected_values() {
        assert_eq!(normalize_incident_status("open"), Some(INCIDENT_STATUS_OPEN));
        assert_eq!(normalize_incident_status(" RESOLVED "), Some(INCIDENT_STATUS_RESOLVED));
        assert_eq!(normalize_incident_status("invalid"), None);
    }

    #[test]
    fn incident_deserializes_legacy_files_without_workflow_fields() {
        let json = r#"{
          "id": "1777100351-v1-4-2",
          "created_at": "2026-04-25T06:58:40.978592Z",
          "severity": "high",
          "summary": "v1.4.2 regression in demo",
          "verdict": {
            "deploy_id": "v1.4.2",
            "environment": "demo",
            "deploy_timestamp": "2026-04-25T06:59:08.239477Z",
            "detected_at": "2026-04-25T06:59:11.239477Z",
            "seconds_after_deploy": 3,
            "error_rate_delta": 0.088,
            "latency_delta_ms": 142.8,
            "reason": "error rate and latency shifted above baseline",
            "top_error_signature": null,
            "top_error_count": 0,
            "top_error_is_new": false,
            "comparison": {
              "baseline_error_rate": 0.012,
              "detected_error_rate": 0.1,
              "baseline_latency_ms": 117.2,
              "detected_latency_ms": 260.0,
              "request_rate_at_detection": 405.0
            },
            "timeline": []
          },
          "alert_text": "watchdog detected a deployment regression"
        }"#;

        let incident: Incident = serde_json::from_str(json).expect("legacy incident should load");

        assert_eq!(incident.status, INCIDENT_STATUS_OPEN);
        assert_eq!(incident.notes, "");
        assert!(incident.cached_explanation.is_none());
        assert!(incident.cached_explanation_updated_at.is_none());
    }
}
