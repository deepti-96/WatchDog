use crate::model::LogEvent;
use anyhow::{Context, Result};
use chrono::Utc;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct LogTailer {
    path: PathBuf,
    offset: u64,
}

impl LogTailer {
    pub fn new(path: PathBuf) -> Self {
        Self { path, offset: 0 }
    }

    pub fn ensure_exists(&self) -> Result<()> {
        if !self.path.exists() {
            File::create(&self.path)
                .with_context(|| format!("failed to create {}", self.path.display()))?;
        }
        Ok(())
    }

    pub fn read_new_events(&mut self) -> Result<Vec<LogEvent>> {
        reset_offset_if_truncated(&self.path, &mut self.offset)?;

        let file = File::open(&self.path)
            .with_context(|| format!("failed to open {}", self.path.display()))?;
        let mut reader = BufReader::new(file);
        reader
            .seek(SeekFrom::Start(self.offset))
            .with_context(|| format!("failed to seek {}", self.path.display()))?;

        let mut events = Vec::new();
        loop {
            let mut line = String::new();
            let bytes = reader.read_line(&mut line)?;
            if bytes == 0 {
                break;
            }

            self.offset = reader.stream_position()?;
            if let Some(event) = parse_log_line(&line) {
                events.push(event);
            }
        }

        Ok(events)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn parse_log_line(line: &str) -> Option<LogEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed).ok();
    }

    let parts = trimmed.splitn(4, ' ').collect::<Vec<_>>();
    if parts.len() >= 4 && looks_like_timestamp(parts[0]) {
        return Some(LogEvent {
            timestamp: chrono::DateTime::parse_from_rfc3339(parts[0]).ok()?.with_timezone(&Utc),
            level: parts[1].to_string(),
            service: parts[2].to_string(),
            message: parts[3].to_string(),
        });
    }

    let parts = trimmed.splitn(3, ' ').collect::<Vec<_>>();
    if parts.len() >= 3 {
        return Some(LogEvent {
            timestamp: Utc::now(),
            level: parts[0].to_string(),
            service: parts[1].to_string(),
            message: parts[2].to_string(),
        });
    }

    None
}

fn looks_like_timestamp(value: &str) -> bool {
    value.contains('T') && value.ends_with('Z')
}

fn reset_offset_if_truncated(path: &Path, offset: &mut u64) -> Result<()> {
    let metadata = fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.len() < *offset {
        *offset = 0;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_log_line() {
        let line = r#"{"timestamp":"2026-03-31T03:10:00Z","level":"ERROR","service":"api","message":"Database timeout"}"#;
        let event = parse_log_line(line).expect("expected json log event");
        assert_eq!(event.level, "ERROR");
        assert_eq!(event.service, "api");
    }

    #[test]
    fn parses_plain_text_log_line() {
        let line = "2026-03-31T03:10:00Z ERROR api Database timeout";
        let event = parse_log_line(line).expect("expected plain text log event");
        assert_eq!(event.level, "ERROR");
        assert_eq!(event.service, "api");
        assert_eq!(event.message, "Database timeout");
    }
}
