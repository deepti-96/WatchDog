use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "watchdog")]
#[command(about = "Detect deployment-caused regressions in seconds.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run {
        #[arg(long, default_value = ".watchdog")]
        state_dir: PathBuf,
        #[arg(long)]
        log_file: Option<PathBuf>,
        #[arg(long, default_value_t = 300)]
        monitoring_window_secs: u64,
        #[arg(long)]
        webhook_url: Option<String>,
    },
    Serve {
        #[arg(long, default_value = ".watchdog")]
        state_dir: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 3000)]
        port: u16,
    },
    Notify {
        #[arg(long, default_value = ".watchdog")]
        state_dir: PathBuf,
        #[arg(long)]
        deploy: String,
        #[arg(long, default_value = "local")]
        environment: String,
    },
    Simulate {
        #[arg(long, default_value = ".watchdog")]
        state_dir: PathBuf,
        #[arg(long)]
        deploy: String,
        #[arg(long, default_value_t = false)]
        bad_deploy: bool,
    },
    Benchmark {
        #[arg(long, default_value_t = 100)]
        trials: usize,
        #[arg(long, default_value_t = 300)]
        monitoring_window_secs: u64,
    },
}
