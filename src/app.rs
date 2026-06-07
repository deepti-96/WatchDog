use crate::alert;
use crate::benchmark;
use crate::cli::{Cli, Command};
use crate::config::WatchdogConfig;
use crate::dashboard;
use crate::engine::{EngineConfig, WatchdogEngine};
use crate::model::{DeployEvent, LogEvent, MetricSample};
use crate::storage;
use crate::tail::LogTailer;
use anyhow::{anyhow, Context, Result};
use chrono::{Duration, SecondsFormat, Utc};
use clap::Parser;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tokio::time::{sleep, Duration as TokioDuration};
use tracing::{info, warn};

pub async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("watchdog=info")
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Run {
            state_dir,
            config,
            log_file,
            monitoring_window_secs,
            webhook_url,
        } => run_daemon(state_dir, config, log_file, monitoring_window_secs, webhook_url).await,
        Command::Serve {
            state_dir,
            host,
            port,
        } => dashboard::serve(state_dir, host, port).await,
        Command::Notify {
            state_dir,
            deploy,
            environment,
        } => notify(state_dir, deploy, environment),
        Command::Simulate {
            state_dir,
            deploy,
            bad_deploy,
        } => simulate(state_dir, deploy, bad_deploy).await,
        Command::Demo {
            state_dir,
            deploy,
            environment,
            healthy,
        } => demo(state_dir, deploy, environment, healthy),
        Command::Benchmark {
            trials,
            monitoring_window_secs,
        } => run_benchmark(trials, monitoring_window_secs),
    }
}

async fn run_daemon(
    state_dir: PathBuf,
    config_path: Option<PathBuf>,
    log_file: Option<PathBuf>,
    monitoring_window_secs: Option<u64>,
    webhook_url: Option<String>,
) -> Result<()> {
    ensure_state_dir(&state_dir)?;
    let settings = WatchdogConfig::load(config_path.as_deref())?
        .resolve_run_settings(log_file, monitoring_window_secs, webhook_url);
    let metrics_path = state_dir.join("metrics.jsonl");
    let deploys_path = state_dir.join("deploy-events.jsonl");
    touch(&metrics_path)?;
    touch(&deploys_path)?;

    let log_path = settings.log_file.unwrap_or_else(|| state_dir.join("app.log"));
    let mut tailer = LogTailer::new(log_path);
    tailer.ensure_exists()?;

    let mut metric_cursor = 0usize;
    let mut deploy_cursor = 0usize;
    let mut engine = WatchdogEngine::with_config(EngineConfig {
        baseline_capacity: settings.baseline_capacity,
        monitoring_window_secs: settings.monitoring_window_secs as i64,
        detector: settings.detector,
    });

    info!("watchdog daemon started");
    info!("state dir: {}", state_dir.display());
    info!("tailing log file: {}", tailer.path().display());
    info!(
        "baseline capacity: {}, monitoring window: {}s",
        settings.baseline_capacity, settings.monitoring_window_secs
    );

    loop {
        for sample in read_new_jsonl::<MetricSample>(&metrics_path, &mut metric_cursor)? {
            if let Some(verdict) = engine.ingest_metric(sample) {
                let message = alert::render(&verdict);
                println!("{message}");
                if let Some(url) = &settings.webhook_url {
                    if let Err(error) = alert::send_webhook(url, &message, &verdict).await {
                        warn!("failed to send webhook alert: {error:#}");
                    }
                }
                if let Err(error) = storage::persist_incident(&state_dir, &verdict, &message) {
                    warn!("failed to persist incident: {error:#}");
                }
            }
        }

        for log in tailer.read_new_events()? {
            engine.ingest_log(log);
        }

        for deploy in read_new_jsonl::<DeployEvent>(&deploys_path, &mut deploy_cursor)? {
            if engine.mark_deploy(deploy.clone()) {
                info!(
                    "armed deploy correlation for {} with baseline of {} samples",
                    deploy.deploy_id,
                    engine.baseline_size()
                );
            } else {
                warn!("ignoring deploy event because baseline is not ready");
            }
        }

        sleep(TokioDuration::from_millis(500)).await;
    }
}

fn notify(state_dir: PathBuf, deploy: String, environment: String) -> Result<()> {
    ensure_state_dir(&state_dir)?;
    let deploys_path = state_dir.join("deploy-events.jsonl");
    let event = DeployEvent {
        timestamp: Utc::now(),
        deploy_id: deploy,
        environment,
    };
    append_jsonl(&deploys_path, &event)?;
    println!(
        "recorded deploy event {} in {}",
        event.deploy_id,
        deploys_path.display()
    );
    Ok(())
}

async fn simulate(state_dir: PathBuf, deploy: String, bad_deploy: bool) -> Result<()> {
    ensure_state_dir(&state_dir)?;
    let metrics_path = state_dir.join("metrics.jsonl");
    let log_path = state_dir.join("app.log");
    let deploys_path = state_dir.join("deploy-events.jsonl");
    let incidents_path = state_dir.join("incidents");
    fs::write(&metrics_path, "")?;
    fs::write(&log_path, "")?;
    fs::write(&deploys_path, "")?;
    if incidents_path.exists() {
        fs::remove_dir_all(&incidents_path)?;
    }

    let start = Utc::now();

    for i in 0..30 {
        append_jsonl(
            &metrics_path,
            &MetricSample {
                timestamp: start + Duration::seconds(i),
                error_rate: 0.01 + ((i % 3) as f64 * 0.002),
                p95_latency_ms: 110.0 + ((i % 4) as f64 * 5.0),
                request_rate: 400.0,
            },
        )?;
    }

    println!("baseline metrics written; waiting for watchdog to ingest them...");
    sleep(TokioDuration::from_millis(1500)).await;

    append_jsonl(
        &deploys_path,
        &DeployEvent {
            timestamp: start + Duration::seconds(31),
            deploy_id: deploy,
            environment: "demo".to_string(),
        },
    )?;

    println!("deploy event written; waiting for watchdog to arm correlation...");
    sleep(TokioDuration::from_millis(1500)).await;

    for i in 32..45 {
        let degraded = bad_deploy && i >= 34;
        append_jsonl(
            &metrics_path,
            &MetricSample {
                timestamp: start + Duration::seconds(i),
                error_rate: if degraded { 0.09 + ((i % 3) as f64 * 0.01) } else { 0.012 },
                p95_latency_ms: if degraded { 260.0 + ((i % 2) as f64 * 30.0) } else { 120.0 },
                request_rate: 405.0,
            },
        )?;

        if degraded {
            append_log_line(
                &log_path,
                &format!(
                    "{} ERROR api Database timeout for user 123 request 8f91ab22",
                    (start + Duration::seconds(i)).to_rfc3339_opts(SecondsFormat::Secs, true)
                ),
            )?;
        }

        sleep(TokioDuration::from_millis(120)).await;
    }

    println!(
        "wrote demo data to {}, {} and {}",
        metrics_path.display(),
        log_path.display(),
        deploys_path.display()
    );
    println!("start the daemon with `cargo run -- run --state-dir {}`", state_dir.display());
    println!("then launch the dashboard with `cargo run -- serve --state-dir {}`", state_dir.display());
    Ok(())
}

fn demo(state_dir: PathBuf, deploy: String, environment: String, healthy: bool) -> Result<()> {
    ensure_state_dir(&state_dir)?;
    reset_demo_state(&state_dir)?;

    let metrics_path = state_dir.join("metrics.jsonl");
    let log_path = state_dir.join("app.log");
    let deploys_path = state_dir.join("deploy-events.jsonl");
    let start = Utc::now();
    let mut engine = WatchdogEngine::new(120, 300);

    for i in 0..30 {
        let sample = MetricSample {
            timestamp: start + Duration::seconds(i),
            error_rate: 0.01 + ((i % 3) as f64 * 0.002),
            p95_latency_ms: 110.0 + ((i % 4) as f64 * 5.0),
            request_rate: 400.0,
        };
        append_jsonl(&metrics_path, &sample)?;
        engine.ingest_metric(sample);
    }

    let deploy_event = DeployEvent {
        timestamp: start + Duration::seconds(31),
        deploy_id: deploy,
        environment,
    };
    append_jsonl(&deploys_path, &deploy_event)?;
    if !engine.mark_deploy(deploy_event.clone()) {
        return Err(anyhow!("demo could not arm deploy correlation"));
    }

    let mut persisted = None;
    for i in 32..45 {
        let erroring = !healthy && i >= 34;
        let degraded = !healthy && i >= 36;
        let timestamp = start + Duration::seconds(i);
        if erroring {
            let message = "Database timeout for user 123 request 8f91ab22";
            append_log_line(
                &log_path,
                &format!(
                    "{} ERROR api {}",
                    timestamp.to_rfc3339_opts(SecondsFormat::Secs, true),
                    message
                ),
            )?;
            engine.ingest_log(LogEvent {
                timestamp,
                level: "ERROR".to_string(),
                service: "api".to_string(),
                message: message.to_string(),
            });
        }

        let sample = MetricSample {
            timestamp,
            error_rate: if degraded { 0.11 + ((i % 3) as f64 * 0.01) } else { 0.012 },
            p95_latency_ms: if degraded { 260.0 + ((i % 2) as f64 * 30.0) } else { 120.0 },
            request_rate: 405.0,
        };
        append_jsonl(&metrics_path, &sample)?;

        if let Some(verdict) = engine.ingest_metric(sample) {
            let message = alert::render(&verdict);
            let incident = storage::persist_incident(&state_dir, &verdict, &message)?;
            println!("{message}");
            persisted = Some(incident);
            break;
        }
    }

    match persisted {
        Some(incident) => {
            println!("demo incident written: {}", incident.id);
            println!(
                "open the dashboard with `cargo run -- serve --state-dir {} --port 3001`",
                state_dir.display()
            );
        }
        None if healthy => {
            println!("healthy demo data written; no incident was expected");
            println!(
                "open the dashboard with `cargo run -- serve --state-dir {} --port 3001`",
                state_dir.display()
            );
        }
        None => return Err(anyhow!("demo did not produce a regression incident")),
    }

    Ok(())
}

fn run_benchmark(trials: usize, monitoring_window_secs: u64) -> Result<()> {
    let summary = benchmark::run(trials, monitoring_window_secs);
    println!("watchdog benchmark summary");
    println!("trials: {}", summary.trials);
    println!("healthy false positives: {}", summary.healthy_false_positives);
    println!("bad deploys detected: {}", summary.bad_detected);
    println!("bad deploys missed: {}", summary.bad_missed);
    println!("average detection latency: {:.2}s", summary.average_detection_secs);
    println!("best detection latency: {}s", summary.best_detection_secs);
    println!("worst detection latency: {}s", summary.worst_detection_secs);
    Ok(())
}

fn ensure_state_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create state dir {}", path.display()))?;
    Ok(())
}

fn touch(path: &Path) -> Result<()> {
    if !path.exists() {
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    }
    Ok(())
}

fn reset_demo_state(state_dir: &Path) -> Result<()> {
    fs::write(state_dir.join("metrics.jsonl"), "")?;
    fs::write(state_dir.join("app.log"), "")?;
    fs::write(state_dir.join("deploy-events.jsonl"), "")?;
    let incidents_path = state_dir.join("incidents");
    if incidents_path.exists() {
        fs::remove_dir_all(&incidents_path)?;
    }
    Ok(())
}

fn append_jsonl<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    serde_json::to_writer(&mut file, value)?;
    writeln!(file)?;
    Ok(())
}

fn append_log_line(path: &Path, line: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn read_new_jsonl<T>(path: &Path, cursor: &mut usize) -> Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let lines = reader.lines().collect::<std::io::Result<Vec<_>>>()?;
    let mut out = Vec::new();

    if *cursor > lines.len() {
        *cursor = 0;
    }

    for line in lines.iter().skip(*cursor) {
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str(&line)?);
    }

    *cursor = lines.len();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    struct JsonlTestRecord {
        value: usize,
    }

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("watchdog-{name}-{}-{nanos}.jsonl", std::process::id()))
    }

    #[test]
    fn read_new_jsonl_counts_blank_lines_in_cursor() {
        let path = temp_path("blank-lines");
        fs::write(&path, "{\"value\":1}\n\n{\"value\":2}\n").expect("write jsonl");

        let mut cursor = 0;
        let records = read_new_jsonl::<JsonlTestRecord>(&path, &mut cursor).expect("read records");
        assert_eq!(records, vec![JsonlTestRecord { value: 1 }, JsonlTestRecord { value: 2 }]);
        assert_eq!(cursor, 3);

        append_jsonl(&path, &JsonlTestRecord { value: 3 }).expect("append record");
        let records = read_new_jsonl::<JsonlTestRecord>(&path, &mut cursor).expect("read appended record");
        assert_eq!(records, vec![JsonlTestRecord { value: 3 }]);
        assert_eq!(cursor, 4);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn read_new_jsonl_resets_cursor_after_file_truncation() {
        let path = temp_path("truncated");
        fs::write(&path, "{\"value\":1}\n{\"value\":2}\n").expect("write jsonl");

        let mut cursor = 0;
        let _ = read_new_jsonl::<JsonlTestRecord>(&path, &mut cursor).expect("read initial records");
        assert_eq!(cursor, 2);

        fs::write(&path, "{\"value\":9}\n").expect("replace jsonl");
        let records = read_new_jsonl::<JsonlTestRecord>(&path, &mut cursor).expect("read replacement record");
        assert_eq!(records, vec![JsonlTestRecord { value: 9 }]);
        assert_eq!(cursor, 1);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn demo_writes_persisted_bad_deploy_incident() {
        let state_dir = temp_path("demo-state");

        demo(
            state_dir.clone(),
            "v-demo".to_string(),
            "demo".to_string(),
            false,
        )
        .expect("demo should persist an incident");

        let incidents = storage::list_incidents(&state_dir).expect("list incidents");
        assert_eq!(incidents.len(), 1);
        assert_eq!(incidents[0].verdict.deploy_id, "v-demo");
        assert!(incidents[0].verdict.top_error_signature.is_some());

        let _ = fs::remove_dir_all(state_dir);
    }
}
