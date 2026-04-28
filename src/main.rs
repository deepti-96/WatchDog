mod alert;
mod app;
mod benchmark;
mod buffer;
mod cli;
mod dashboard;
mod detector;
mod engine;
mod export;
mod llm;
mod logs;
mod model;
mod storage;
mod tail;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    app::run().await
}
