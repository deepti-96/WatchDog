mod alert;
mod app;
mod benchmark;
mod buffer;
mod cli;
mod detector;
mod engine;
mod model;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    app::run().await
}
