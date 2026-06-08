use crate::model::{
    normalize_incident_status, Incident, RegressionVerdict, INCIDENT_STATUS_OPEN,
};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::env;
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
    if storage_backend().is_sqlite() {
        return list_incidents_sqlite(state_dir);
    }

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
    if storage_backend().is_sqlite() {
        return read_incident_sqlite(state_dir, incident_id);
    }

    let path = incident_path(state_dir, incident_id);
    if !path.exists() {
        return Ok(None);
    }
    let file = File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    let incident = serde_json::from_reader(file)?;
    Ok(Some(incident))
}

pub fn storage_backend_label() -> &'static str {
    match storage_backend() {
        StorageBackend::Sqlite => "sqlite",
        StorageBackend::JsonFiles => "json-files",
    }
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
    if storage_backend().is_sqlite() {
        return write_incident_sqlite(state_dir, incident);
    }

    let incidents_dir = incidents_dir(state_dir);
    fs::create_dir_all(&incidents_dir)
        .with_context(|| format!("failed to create incidents dir {}", incidents_dir.display()))?;
    let path = incident_path(state_dir, &incident.id);
    let file = File::create(&path).with_context(|| format!("failed to create {}", path.display()))?;
    serde_json::to_writer_pretty(file, incident)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorageBackend {
    JsonFiles,
    Sqlite,
}

impl StorageBackend {
    fn is_sqlite(self) -> bool {
        matches!(self, StorageBackend::Sqlite)
    }
}

fn storage_backend() -> StorageBackend {
    match env::var("WATCHDOG_STORAGE") {
        Ok(value) if value.eq_ignore_ascii_case("sqlite") => StorageBackend::Sqlite,
        Ok(value) if value.eq_ignore_ascii_case("db") => StorageBackend::Sqlite,
        Ok(value) if value.eq_ignore_ascii_case("database") => StorageBackend::Sqlite,
        _ => StorageBackend::JsonFiles,
    }
}

fn sqlite_path(state_dir: &Path) -> PathBuf {
    if let Ok(value) = env::var("WATCHDOG_DATABASE_URL") {
        if let Some(path) = value.strip_prefix("sqlite:") {
            return PathBuf::from(path);
        }
        if let Some(path) = value.strip_prefix("sqlite://") {
            return PathBuf::from(path);
        }
        return PathBuf::from(value);
    }

    state_dir.join("watchdog.sqlite")
}

fn sqlite_connection(state_dir: &Path) -> Result<Connection> {
    fs::create_dir_all(state_dir)
        .with_context(|| format!("failed to create state dir {}", state_dir.display()))?;

    let path = sqlite_path(state_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create sqlite dir {}", parent.display()))?;
    }

    let connection = Connection::open(&path)
        .with_context(|| format!("failed to open sqlite database {}", path.display()))?;
    connection.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS incidents (
            id TEXT PRIMARY KEY,
            created_at TEXT NOT NULL,
            severity TEXT NOT NULL,
            status TEXT NOT NULL,
            deploy_id TEXT NOT NULL,
            environment TEXT NOT NULL,
            summary TEXT NOT NULL,
            incident_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_incidents_created_at ON incidents(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_incidents_status ON incidents(status);
        CREATE INDEX IF NOT EXISTS idx_incidents_deploy_id ON incidents(deploy_id);
        "#,
    )?;
    Ok(connection)
}

fn write_incident_sqlite(state_dir: &Path, incident: &Incident) -> Result<()> {
    let connection = sqlite_connection(state_dir)?;
    let incident_json = serde_json::to_string(incident)?;
    connection.execute(
        r#"
        INSERT INTO incidents (
            id, created_at, severity, status, deploy_id, environment, summary, incident_json, updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(id) DO UPDATE SET
            created_at = excluded.created_at,
            severity = excluded.severity,
            status = excluded.status,
            deploy_id = excluded.deploy_id,
            environment = excluded.environment,
            summary = excluded.summary,
            incident_json = excluded.incident_json,
            updated_at = excluded.updated_at
        "#,
        params![
            &incident.id,
            incident.created_at.to_rfc3339(),
            &incident.severity,
            &incident.status,
            &incident.verdict.deploy_id,
            &incident.verdict.environment,
            &incident.summary,
            incident_json,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn list_incidents_sqlite(state_dir: &Path) -> Result<Vec<Incident>> {
    let connection = sqlite_connection(state_dir)?;
    let mut statement = connection.prepare(
        "SELECT incident_json FROM incidents ORDER BY created_at DESC",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;

    let mut incidents = Vec::new();
    for row in rows {
        let incident_json = row?;
        incidents.push(serde_json::from_str::<Incident>(&incident_json)?);
    }
    Ok(incidents)
}

fn read_incident_sqlite(state_dir: &Path, incident_id: &str) -> Result<Option<Incident>> {
    let connection = sqlite_connection(state_dir)?;
    let mut statement = connection.prepare(
        "SELECT incident_json FROM incidents WHERE id = ?1 LIMIT 1",
    )?;
    let mut rows = statement.query(params![incident_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let incident_json: String = row.get(0)?;
    Ok(Some(serde_json::from_str(&incident_json)?))
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
