//! Change Intelligence Worker - A standalone service for Git repository diff analysis using gix.
//!
//! This worker receives change analysis jobs from a coordinator, materializes repository state,
//! computes commit-to-commit changes, and returns structured change intelligence.
//!
//! ## Architecture
//!
//! The worker is stateless and communicates with a coordinator service:
//!
//! ```text
//!           Coordinator
//!               |
//!         Work Queue / RPC
//!               |
//!               v
//!   change-intelligence-worker
//!               |
//!               v
//!             gix
//!               |
//!               v
//!       ChangeSet + Impact
//! ```
//!
//! ## Environment Variables
//!
//! - `COORDINATOR_URL`: URL of the coordinator service (required)
//! - `WORKER_ID`: Unique identifier for this worker (default: "gix-worker")
//! - `REPOSITORY_CACHE`: Path to repository cache directory (default: "/data/repos")
//! - `HEALTH_PORT`: Port for health check endpoint (default: 8080)
//! - `POLL_INTERVAL_SECS`: Job polling interval in seconds (default: 2)

#![deny(unsafe_code, rust_2018_idioms)]
#![forbid(unsafe_code)]

mod cache;
mod config;
mod gix_engine;
mod models;
mod worker;

use anyhow::Result;
use tracing::info;

use config::Config;
use worker::Worker;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting Change Intelligence Worker"
    );

    // Load configuration from environment
    let config = Config::from_env();

    info!(
        worker_id = %config.worker_id,
        coordinator = %config.coordinator_url,
        cache = %config.repository_cache,
        health_port = config.health_port,
        "Configuration loaded"
    );

    // Create and run the worker
    let worker = Worker::new(config);
    worker.run().await?;

    Ok(())
}
