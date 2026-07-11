//! Data models for change intelligence jobs and results.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A job request for analyzing changes between commits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeAnalysisJob {
    /// Unique identifier for this job.
    pub job_id: Uuid,
    /// URL of the repository to analyze.
    pub repository_url: String,
    /// The base commit (from).
    pub base_commit: String,
    /// The head commit (to).
    pub head_commit: String,
    /// Optional specific paths to analyze.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// Result of a change analysis job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeAnalysisResult {
    /// The job ID this result corresponds to.
    pub job_id: Uuid,
    /// Whether the analysis completed successfully.
    pub success: bool,
    /// Error message if the analysis failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The change set computed from the analysis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_set: Option<ChangeSet>,
}

impl ChangeAnalysisResult {
    /// Create a successful result with a change set.
    pub fn success(job_id: Uuid, change_set: ChangeSet) -> Self {
        Self {
            job_id,
            success: true,
            error: None,
            change_set: Some(change_set),
        }
    }

    /// Create a failed result with an error message.
    pub fn failure(job_id: Uuid, error: impl Into<String>) -> Self {
        Self {
            job_id,
            success: false,
            error: Some(error.into()),
            change_set: None,
        }
    }
}

/// A set of changes between two commits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSet {
    /// The base commit SHA.
    pub base_commit: String,
    /// The head commit SHA.
    pub head_commit: String,
    /// List of file changes.
    pub files: Vec<FileChange>,
    /// Summary statistics.
    pub summary: ChangeSummary,
}

/// A change to a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    /// Path to the file.
    pub path: String,
    /// Type of change.
    pub change_type: ChangeType,
    /// Number of lines added.
    pub additions: u32,
    /// Number of lines deleted.
    pub deletions: u32,
    /// Classification of the change impact.
    pub impact: ChangeImpact,
}

/// Type of change to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    /// File was added.
    Added,
    /// File was modified.
    Modified,
    /// File was deleted.
    Deleted,
    /// File was renamed.
    Renamed,
    /// File was copied.
    Copied,
}

/// Classification of change impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeImpact {
    /// Low impact change (documentation, comments, etc.).
    Low,
    /// Medium impact change (tests, configuration, etc.).
    Medium,
    /// High impact change (core logic, API changes, etc.).
    High,
    /// Critical impact change (security, breaking changes, etc.).
    Critical,
}

/// Summary statistics for a change set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSummary {
    /// Total number of files changed.
    pub files_changed: u32,
    /// Total number of additions.
    pub total_additions: u32,
    /// Total number of deletions.
    pub total_deletions: u32,
    /// Number of files by change type.
    pub by_change_type: ChangeTypeCounts,
    /// Number of changes by impact level.
    pub by_impact: ImpactCounts,
}

/// Counts by change type.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeTypeCounts {
    pub added: u32,
    pub modified: u32,
    pub deleted: u32,
    pub renamed: u32,
    pub copied: u32,
}

/// Counts by impact level.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImpactCounts {
    pub low: u32,
    pub medium: u32,
    pub high: u32,
    pub critical: u32,
}

/// Worker registration request sent to the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRegistration {
    /// Unique identifier for this worker.
    pub worker_id: String,
    /// List of capabilities this worker provides.
    pub capabilities: Vec<String>,
    /// Version of the worker.
    pub version: String,
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Status of the worker.
    pub status: String,
    /// Primary capability of the worker.
    pub capability: String,
    /// Provider implementation.
    pub provider: String,
}

impl Default for HealthResponse {
    fn default() -> Self {
        Self {
            status: "ok".to_string(),
            capability: "change-intelligence".to_string(),
            provider: "gix".to_string(),
        }
    }
}
