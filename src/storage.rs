use crate::model::{
    normalize_incident_status, Incident, RegressionVerdict, INCIDENT_STATUS_OPEN,
};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

pub fn persist_incident(state_dir: &Path, verdict: &RegressionVerdict, alert_text: &str) -> Result<Incident> {
    let incident = Incident {
        id: build_incident_id(verdict),
        created_at: Utc::now(),
        severity: severity_for(verdict),
        summary: summary_for(verdict),
        verdict: verdict.clone(),
        alert_text: alert_text.to_string(),
        cached_explanation: None,
        cached_explanation_updated_at: None,
        status: INCIDENT_STATUS_OPEN.to_string(),
        notes: String::new(),
    };

    write_incident(state_dir, &incident)?;
    Ok(incident)
}

pub fn list_incidents(state_dir: &Path) -> Result<Vec<Incident>> {
    let dir = incidents_dir(state_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut incidents = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let file = File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        let incident: Incident = serde_json::from_reader(file)?;
        incidents.push(incident);
    }

    incidents.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(incidents)
}

pub fn read_incident(state_dir: &Path, incident_id: &str) -> Result<Option<Incident>> {
    let path = incident_path(state_dir, incident_id);
    if !path.exists() {
        return Ok(None);
    }
    let file = File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    let incident = serde_json::from_reader(file)?;
    Ok(Some(incident))
}

pub fn update_incident_explanation(state_dir: &Path, incident_id: &str, explanation: &str) -> Result<Option<Incident>> {
    let Some(mut incident) = read_incident(state_dir, incident_id)? else {
        return Ok(None);
    };

    incident.cached_explanation = Some(explanation.to_string());
    incident.cached_explanation_updated_at = Some(Utc::now());
    write_incident(state_dir, &incident)?;
    Ok(Some(incident))
}

pub fn update_incident_status(state_dir: &Path, incident_id: &str, status: &str) -> Result<Option<Incident>> {
    let Some(mut incident) = read_incident(state_dir, incident_id)? else {
        return Ok(None);
    };

    let normalized = normalize_incident_status(status)
        .ok_or_else(|| anyhow!("invalid incident status: {status}"))?;
    incident.status = normalized.to_string();
    write_incident(state_dir, &incident)?;
    Ok(Some(incident))
}

pub fn update_incident_notes(state_dir: &Path, incident_id: &str, notes: &str) -> Result<Option<Incident>> {
    let Some(mut incident) = read_incident(state_dir, incident_id)? else {
        return Ok(None);
    };

    incident.notes = notes.trim().to_string();
    write_incident(state_dir, &incident)?;
    Ok(Some(incident))
}

fn write_incident(state_dir: &Path, incident: &Incident) -> Result<()> {
    let incidents_dir = incidents_dir(state_dir);
    fs::create_dir_all(&incidents_dir)
        .with_context(|| format!("failed to create incidents dir {}", incidents_dir.display()))?;
    let path = incident_path(state_dir, &incident.id);
    let file = File::create(&path).with_context(|| format!("failed to create {}", path.display()))?;
    serde_json::to_writer_pretty(file, incident)?;
    Ok(())
}

fn severity_for(verdict: &RegressionVerdict) -> String {
    if verdict.error_rate_delta >= 0.08 || verdict.latency_delta_ms >= 150.0 {
        "high".to_string()
    } else {
        "medium".to_string()
    }
}

fn summary_for(verdict: &RegressionVerdict) -> String {
    match &verdict.top_error_signature {
        Some(signature) => format!(
            "{} regression in {} with dominant error '{}'",
            verdict.deploy_id, verdict.environment, signature
        ),
        None => format!(
            "{} regression in {} driven by {}",
            verdict.deploy_id, verdict.environment, verdict.reason
        ),
    }
}

fn build_incident_id(verdict: &RegressionVerdict) -> String {
    let slug = verdict
        .deploy_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' })
        .collect::<String>();
    format!("{}-{}", verdict.detected_at.timestamp(), slug)
}

fn incidents_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("incidents")
}

fn incident_path(state_dir: &Path, incident_id: &str) -> PathBuf {
    incidents_dir(state_dir).join(format!("{}.json", incident_id))
}
