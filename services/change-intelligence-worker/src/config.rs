//! Environment configuration for the change intelligence worker.

use std::env;

/// Configuration for the change intelligence worker.
#[derive(Debug, Clone)]
pub struct Config {
    /// URL of the coordinator service for job coordination.
    pub coordinator_url: String,
    /// Unique identifier for this worker instance.
    pub worker_id: String,
    /// Path to the repository cache directory.
    pub repository_cache: String,
    /// Port on which to run the health check server.
    pub health_port: u16,
    /// Polling interval in seconds for checking new jobs.
    pub poll_interval_secs: u64,
}

impl Config {
    /// Load configuration from environment variables.
    ///
    /// # Panics
    ///
    /// Panics if required environment variables are not set.
    pub fn from_env() -> Self {
        Self {
            coordinator_url: env::var("COORDINATOR_URL")
                .expect("COORDINATOR_URL environment variable is required"),
            worker_id: env::var("WORKER_ID").unwrap_or_else(|_| "gix-worker".into()),
            repository_cache: env::var("REPOSITORY_CACHE").unwrap_or_else(|_| "/data/repos".into()),
            health_port: env::var("HEALTH_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
            poll_interval_secs: env::var("POLL_INTERVAL_SECS")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(2),
        }
    }
}
