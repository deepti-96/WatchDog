use crate::model::LogEvent;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone)]
pub struct ErrorRingBuffer {
    cap: usize,
    items: VecDeque<String>,
    counts: HashMap<String, usize>,
}

impl ErrorRingBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            items: VecDeque::with_capacity(cap),
            counts: HashMap::new(),
        }
    }

    pub fn push(&mut self, signature: String) {
        if self.items.len() == self.cap {
            if let Some(oldest) = self.items.pop_front() {
                decrement_count(&mut self.counts, &oldest);
            }
        }

        self.items.push_back(signature.clone());
        *self.counts.entry(signature).or_insert(0) += 1;
    }

    pub fn snapshot_counts(&self) -> HashMap<String, usize> {
        self.counts.clone()
    }
}

pub fn extract_error_signature(event: &LogEvent) -> Option<String> {
    if !event.level.eq_ignore_ascii_case("error") {
        return None;
    }

    let normalized_tokens = event
        .message
        .split_whitespace()
        .take(16)
        .map(normalize_token)
        .collect::<Vec<_>>();

    if normalized_tokens.is_empty() {
        return None;
    }

    Some(format!("{}: {}", event.service, normalized_tokens.join(" ")))
}

fn normalize_token(token: &str) -> String {
    let trimmed = token
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '.');

    if trimmed.is_empty() {
        return "<empty>".to_string();
    }

    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return "<num>".to_string();
    }

    if trimmed.len() >= 8 && trimmed.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return "<id>".to_string();
    }

    trimmed
        .chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c.to_ascii_lowercase() })
        .collect()
}

fn decrement_count(counts: &mut HashMap<String, usize>, signature: &str) {
    if let Some(count) = counts.get_mut(signature) {
        *count -= 1;
        if *count == 0 {
            counts.remove(signature);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn error_signature_normalizes_ids_and_numbers() {
        let event = LogEvent {
            timestamp: Utc::now(),
            level: "ERROR".to_string(),
            service: "api".to_string(),
            message: "Database timeout for user 123 request 8f91ab22".to_string(),
        };

        let signature = extract_error_signature(&event).unwrap();
        assert!(signature.contains("api:"));
        assert!(signature.contains("<num>"));
        assert!(signature.contains("<id>"));
    }
}
