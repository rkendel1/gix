//! Worker behavior and job processing logic.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::gix_engine;
use crate::models::{
    ChangeAnalysisJob, ChangeAnalysisResult, HealthResponse, WorkerRegistration,
};

/// State shared across the worker and health check server.
#[derive(Debug)]
pub struct WorkerState {
    /// Current status of the worker.
    pub status: RwLock<WorkerStatus>,
    /// Worker configuration (retained for future extensions).
    #[allow(dead_code)]
    pub config: Config,
}

/// Current status of the worker.
#[derive(Debug, Clone, Default)]
pub struct WorkerStatus {
    /// Number of jobs processed.
    pub jobs_processed: u64,
    /// Number of jobs failed.
    pub jobs_failed: u64,
    /// Whether the worker is currently processing a job.
    pub is_busy: bool,
}

/// The main worker service.
pub struct Worker {
    config: Config,
    http_client: reqwest::Client,
    state: Arc<WorkerState>,
}

impl Worker {
    /// Create a new worker with the given configuration.
    pub fn new(config: Config) -> Self {
        let state = Arc::new(WorkerState {
            status: RwLock::new(WorkerStatus::default()),
            config: config.clone(),
        });

        Self {
            config,
            http_client: reqwest::Client::new(),
            state,
        }
    }

    /// Run the worker, starting both the job loop and health check server.
    pub async fn run(&self) -> Result<()> {
        // Register with the coordinator
        self.register().await?;

        // Start the health check server in a separate task
        let health_state = Arc::clone(&self.state);
        let health_port = self.config.health_port;
        tokio::spawn(async move {
            if let Err(e) = run_health_server(health_state, health_port).await {
                error!("Health server error: {e}");
            }
        });

        // Run the main job processing loop
        self.job_loop().await
    }

    /// Register this worker with the coordinator.
    async fn register(&self) -> Result<()> {
        let registration = WorkerRegistration {
            worker_id: self.config.worker_id.clone(),
            capabilities: vec!["git.diff".to_string(), "change.intelligence".to_string()],
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let url = format!("{}/api/workers/register", self.config.coordinator_url);

        info!(worker_id = %registration.worker_id, "Registering with coordinator");

        match self.http_client.post(&url).json(&registration).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    info!("Successfully registered with coordinator");
                } else {
                    warn!(
                        status = %response.status(),
                        "Registration returned non-success status"
                    );
                }
            }
            Err(e) => {
                // Log but don't fail - coordinator might not be available yet
                warn!("Failed to register with coordinator: {e}");
            }
        }

        Ok(())
    }

    /// Main job processing loop.
    async fn job_loop(&self) -> Result<()> {
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let cache_path = PathBuf::from(&self.config.repository_cache);

        loop {
            match self.claim_job().await {
                Ok(Some(job)) => {
                    info!(job_id = %job.job_id, "Claimed job");

                    // Mark as busy
                    {
                        let mut status = self.state.status.write().await;
                        status.is_busy = true;
                    }

                    // Process the job
                    let result = self.process_job(&job, &cache_path).await;

                    // Update status
                    {
                        let mut status = self.state.status.write().await;
                        status.is_busy = false;
                        if result.success {
                            status.jobs_processed += 1;
                        } else {
                            status.jobs_failed += 1;
                        }
                    }

                    // Report result to coordinator
                    if let Err(e) = self.complete_job(&result).await {
                        error!(job_id = %job.job_id, "Failed to report job completion: {e}");
                    }
                }
                Ok(None) => {
                    // No job available, wait before polling again
                    tokio::time::sleep(poll_interval).await;
                }
                Err(e) => {
                    error!("Failed to claim job: {e}");
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }

    /// Attempt to claim a job from the coordinator.
    async fn claim_job(&self) -> Result<Option<ChangeAnalysisJob>> {
        let url = format!(
            "{}/api/jobs/claim?worker_id={}&capability=change.intelligence",
            self.config.coordinator_url, self.config.worker_id
        );

        let response = self
            .http_client
            .post(&url)
            .send()
            .await
            .context("Failed to contact coordinator")?;

        if response.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }

        if !response.status().is_success() {
            anyhow::bail!("Coordinator returned error: {}", response.status());
        }

        let job = response
            .json::<ChangeAnalysisJob>()
            .await
            .context("Failed to parse job")?;

        Ok(Some(job))
    }

    /// Process a single job and return the result.
    async fn process_job(
        &self,
        job: &ChangeAnalysisJob,
        cache_path: &Path,
    ) -> ChangeAnalysisResult {
        match gix_engine::analyze(job, cache_path).await {
            Ok(change_set) => {
                info!(
                    job_id = %job.job_id,
                    files_changed = change_set.files.len(),
                    "Job completed successfully"
                );
                ChangeAnalysisResult::success(job.job_id, change_set)
            }
            Err(e) => {
                error!(job_id = %job.job_id, error = %e, "Job failed");
                ChangeAnalysisResult::failure(job.job_id, e.to_string())
            }
        }
    }

    /// Report job completion to the coordinator.
    async fn complete_job(&self, result: &ChangeAnalysisResult) -> Result<()> {
        let url = format!("{}/api/jobs/complete", self.config.coordinator_url);

        let response = self
            .http_client
            .post(&url)
            .json(result)
            .send()
            .await
            .context("Failed to contact coordinator")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Coordinator returned error on completion: {}",
                response.status()
            );
        }

        Ok(())
    }
}

/// Run the health check HTTP server.
async fn run_health_server(state: Arc<WorkerState>, port: u16) -> Result<()> {
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    info!("Health server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Health check endpoint handler.
async fn health_handler(State(_state): State<Arc<WorkerState>>) -> Json<HealthResponse> {
    Json(HealthResponse::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_status_default() {
        let status = WorkerStatus::default();
        assert_eq!(status.jobs_processed, 0);
        assert_eq!(status.jobs_failed, 0);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_health_response_default() {
        let response = HealthResponse::default();
        assert_eq!(response.status, "ok");
        assert_eq!(response.capability, "change-intelligence");
        assert_eq!(response.provider, "gix");
    }
}
