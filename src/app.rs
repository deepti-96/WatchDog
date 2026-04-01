use crate::alert;
use crate::benchmark;
use crate::cli::{Cli, Command};
use crate::engine::WatchdogEngine;
use crate::model::{DeployEvent, MetricSample};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
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
            monitoring_window_secs,
            webhook_url,
        } => run_daemon(state_dir, monitoring_window_secs, webhook_url).await,
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
        Command::Benchmark {
            trials,
            monitoring_window_secs,
        } => run_benchmark(trials, monitoring_window_secs),
    }
}

async fn run_daemon(
    state_dir: PathBuf,
    monitoring_window_secs: u64,
    webhook_url: Option<String>,
) -> Result<()> {
    ensure_state_dir(&state_dir)?;
    let metrics_path = state_dir.join("metrics.jsonl");
    let deploys_path = state_dir.join("deploy-events.jsonl");
    touch(&metrics_path)?;
    touch(&deploys_path)?;

    let mut metric_cursor = 0usize;
    let mut deploy_cursor = 0usize;
    let mut engine = WatchdogEngine::new(120, monitoring_window_secs as i64);

    info!("watchdog daemon started");
    info!("state dir: {}", state_dir.display());

    loop {
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

        for sample in read_new_jsonl::<MetricSample>(&metrics_path, &mut metric_cursor)? {
            if let Some(verdict) = engine.ingest_metric(sample) {
                let message = alert::render(&verdict);
                println!("{message}");
                if let Some(url) = &webhook_url {
                    if let Err(error) = alert::send_webhook(url, &message).await {
                        warn!("failed to send webhook alert: {error:#}");
                    }
                }
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
    let deploys_path = state_dir.join("deploy-events.jsonl");
    fs::write(&metrics_path, "")?;
    fs::write(&deploys_path, "")?;

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

    append_jsonl(
        &deploys_path,
        &DeployEvent {
            timestamp: start + Duration::seconds(31),
            deploy_id: deploy,
            environment: "demo".to_string(),
        },
    )?;

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
    }

    println!(
        "wrote demo data to {} and {}",
        metrics_path.display(),
        deploys_path.display()
    );
    println!("start the daemon with `cargo run -- run --state-dir {}`", state_dir.display());
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

fn read_new_jsonl<T>(path: &Path, cursor: &mut usize) -> Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        if index < *cursor {
            continue;
        }
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str(&line)?);
    }

    *cursor += out.len();
    Ok(out)
}
