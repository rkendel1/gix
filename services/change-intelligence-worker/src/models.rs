//! Data models for change intelligence jobs and results.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Schema version for change intelligence output contract.
/// Increment this when the output format changes in a breaking way.
pub const CHANGE_INTELLIGENCE_VERSION: &str = "v1";

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

/// Metadata about the worker that produced an analysis result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerMetadata {
    /// Unique identifier for this worker instance.
    pub worker_id: String,
    /// Provider implementation (e.g., "gix").
    pub provider: String,
    /// Engine used for analysis (e.g., "gitoxide").
    pub engine: String,
    /// Version of the worker/engine.
    pub version: String,
}

impl WorkerMetadata {
    /// Create a new WorkerMetadata with the given worker ID.
    pub fn new(worker_id: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
            provider: "gix".to_string(),
            engine: "gitoxide".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Identity of the repository that was analyzed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryIdentity {
    /// URL of the repository.
    pub repository_url: String,
    /// The base commit SHA that was analyzed.
    pub base_commit: String,
    /// The target/head commit SHA that was analyzed.
    pub target_commit: String,
    /// Tree hash of the target commit (for verification).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree_hash: Option<String>,
}

/// Result of a change analysis job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeAnalysisResult {
    /// Schema version of this result format.
    pub schema_version: String,
    /// The job ID this result corresponds to.
    pub job_id: Uuid,
    /// Whether the analysis completed successfully.
    pub success: bool,
    /// Error message if the analysis failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Metadata about the worker that produced this result.
    pub worker: WorkerMetadata,
    /// Identity of the repository that was analyzed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryIdentity>,
    /// The change set computed from the analysis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_set: Option<ChangeSet>,
    /// Impact classification of the changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<ChangeImpactSummary>,
    /// SHA256 hash of the canonical change set JSON for determinism verification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_hash: Option<String>,
}

impl ChangeAnalysisResult {
    /// Create a successful result with a change set.
    pub fn success(
        job_id: Uuid,
        worker_id: &str,
        repository: RepositoryIdentity,
        change_set: ChangeSet,
    ) -> Self {
        let impact = ChangeImpactSummary::from_change_set(&change_set);
        let artifact_hash = compute_artifact_hash(&change_set);

        Self {
            schema_version: CHANGE_INTELLIGENCE_VERSION.to_string(),
            job_id,
            success: true,
            error: None,
            worker: WorkerMetadata::new(worker_id),
            repository: Some(repository),
            change_set: Some(change_set),
            impact: Some(impact),
            artifact_hash: Some(artifact_hash),
        }
    }

    /// Create a failed result with an error message.
    pub fn failure(job_id: Uuid, worker_id: &str, error: impl Into<String>) -> Self {
        Self {
            schema_version: CHANGE_INTELLIGENCE_VERSION.to_string(),
            job_id,
            success: false,
            error: Some(error.into()),
            worker: WorkerMetadata::new(worker_id),
            repository: None,
            change_set: None,
            impact: None,
            artifact_hash: None,
        }
    }
}

/// Compute a deterministic SHA256 hash of the change set.
/// This ensures reproducibility - same input always produces same hash.
pub fn compute_artifact_hash(change_set: &ChangeSet) -> String {
    // Sort files by path for deterministic ordering
    let mut sorted_change_set = change_set.clone();
    sorted_change_set.files.sort_by(|a, b| a.path.cmp(&b.path));

    // Serialize to canonical JSON (sorted keys are handled by serde by default)
    let canonical_json = serde_json::to_string(&sorted_change_set)
        .expect("ChangeSet should always serialize to JSON");

    // Compute SHA256
    let mut hasher = Sha256::new();
    hasher.update(canonical_json.as_bytes());
    let hash = hasher.finalize();

    format!("sha256:{hash:x}")
}

/// Summary of change impact across the entire change set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeImpactSummary {
    /// Highest impact level in the change set.
    pub highest_impact: ChangeImpact,
    /// Whether the changes include any critical impact changes.
    pub has_critical: bool,
    /// Whether the changes include security-related files.
    pub has_security_related: bool,
    /// Summary of impact distribution.
    pub distribution: ImpactCounts,
}

impl ChangeImpactSummary {
    /// Create an impact summary from a change set.
    pub fn from_change_set(change_set: &ChangeSet) -> Self {
        let distribution = change_set.summary.by_impact.clone();

        let has_critical = distribution.critical > 0;
        let has_security_related = change_set.files.iter().any(|f| {
            let path = f.path.to_lowercase();
            path.contains("security") || path.contains("auth") || path.contains("secret")
        });

        let highest_impact = if distribution.critical > 0 {
            ChangeImpact::Critical
        } else if distribution.high > 0 {
            ChangeImpact::High
        } else if distribution.medium > 0 {
            ChangeImpact::Medium
        } else {
            ChangeImpact::Low
        };

        Self {
            highest_impact,
            has_critical,
            has_security_related,
            distribution,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactCounts {
    pub low: u32,
    pub medium: u32,
    pub high: u32,
    pub critical: u32,
}

/// A versioned capability declaration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capability {
    /// Name of the capability (e.g., "git.diff", "change.intelligence").
    pub name: String,
    /// Version of this capability.
    pub version: String,
}

impl Capability {
    /// Create a new capability declaration.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// Worker registration request sent to the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRegistration {
    /// Unique identifier for this worker.
    pub worker_id: String,
    /// List of versioned capabilities this worker provides.
    pub capabilities: Vec<Capability>,
    /// Version of the worker.
    pub version: String,
}

impl WorkerRegistration {
    /// Create a new worker registration with standard capabilities.
    pub fn new(worker_id: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
            capabilities: vec![
                Capability::new("git.diff", "1"),
                Capability::new("change.intelligence", "1"),
            ],
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Health check response with extended worker state information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Status of the worker ("ok", "degraded", "error").
    pub status: String,
    /// Primary capability of the worker.
    pub capability: String,
    /// Provider implementation.
    pub provider: String,
    /// Version of the worker.
    pub version: String,
    /// Number of jobs successfully processed.
    pub jobs_processed: u64,
    /// Number of repositories cached.
    pub cache_repositories: u64,
}

impl Default for HealthResponse {
    fn default() -> Self {
        Self {
            status: "ok".to_string(),
            capability: "change-intelligence".to_string(),
            provider: "gix".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            jobs_processed: 0,
            cache_repositories: 0,
        }
    }
}

impl HealthResponse {
    /// Create a health response with current worker state.
    pub fn with_state(jobs_processed: u64, cache_repositories: u64) -> Self {
        Self {
            jobs_processed,
            cache_repositories,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_hash_is_deterministic() {
        let change_set = ChangeSet {
            base_commit: "abc123".to_string(),
            head_commit: "def456".to_string(),
            files: vec![
                FileChange {
                    path: "src/main.rs".to_string(),
                    change_type: ChangeType::Modified,
                    additions: 10,
                    deletions: 5,
                    impact: ChangeImpact::High,
                },
                FileChange {
                    path: "README.md".to_string(),
                    change_type: ChangeType::Modified,
                    additions: 2,
                    deletions: 1,
                    impact: ChangeImpact::Low,
                },
            ],
            summary: ChangeSummary {
                files_changed: 2,
                total_additions: 12,
                total_deletions: 6,
                by_change_type: ChangeTypeCounts {
                    modified: 2,
                    ..Default::default()
                },
                by_impact: ImpactCounts {
                    high: 1,
                    low: 1,
                    ..Default::default()
                },
            },
        };

        // Run hash computation multiple times
        let hash1 = compute_artifact_hash(&change_set);
        let hash2 = compute_artifact_hash(&change_set);
        let hash3 = compute_artifact_hash(&change_set);

        // All hashes should be identical
        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);

        // Hash should start with "sha256:"
        assert!(hash1.starts_with("sha256:"));
    }

    #[test]
    fn test_artifact_hash_ordering_independence() {
        // Create two change sets with files in different orders
        let change_set_a = ChangeSet {
            base_commit: "abc123".to_string(),
            head_commit: "def456".to_string(),
            files: vec![
                FileChange {
                    path: "src/main.rs".to_string(),
                    change_type: ChangeType::Modified,
                    additions: 10,
                    deletions: 5,
                    impact: ChangeImpact::High,
                },
                FileChange {
                    path: "README.md".to_string(),
                    change_type: ChangeType::Modified,
                    additions: 2,
                    deletions: 1,
                    impact: ChangeImpact::Low,
                },
            ],
            summary: ChangeSummary {
                files_changed: 2,
                total_additions: 12,
                total_deletions: 6,
                by_change_type: ChangeTypeCounts {
                    modified: 2,
                    ..Default::default()
                },
                by_impact: ImpactCounts {
                    high: 1,
                    low: 1,
                    ..Default::default()
                },
            },
        };

        let change_set_b = ChangeSet {
            base_commit: "abc123".to_string(),
            head_commit: "def456".to_string(),
            files: vec![
                // Files in different order
                FileChange {
                    path: "README.md".to_string(),
                    change_type: ChangeType::Modified,
                    additions: 2,
                    deletions: 1,
                    impact: ChangeImpact::Low,
                },
                FileChange {
                    path: "src/main.rs".to_string(),
                    change_type: ChangeType::Modified,
                    additions: 10,
                    deletions: 5,
                    impact: ChangeImpact::High,
                },
            ],
            summary: ChangeSummary {
                files_changed: 2,
                total_additions: 12,
                total_deletions: 6,
                by_change_type: ChangeTypeCounts {
                    modified: 2,
                    ..Default::default()
                },
                by_impact: ImpactCounts {
                    high: 1,
                    low: 1,
                    ..Default::default()
                },
            },
        };

        // Hashes should be identical regardless of file order
        let hash_a = compute_artifact_hash(&change_set_a);
        let hash_b = compute_artifact_hash(&change_set_b);
        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn test_change_impact_summary() {
        let change_set = ChangeSet {
            base_commit: "abc123".to_string(),
            head_commit: "def456".to_string(),
            files: vec![
                FileChange {
                    path: "src/security/auth.rs".to_string(),
                    change_type: ChangeType::Modified,
                    additions: 10,
                    deletions: 5,
                    impact: ChangeImpact::Critical,
                },
            ],
            summary: ChangeSummary {
                files_changed: 1,
                total_additions: 10,
                total_deletions: 5,
                by_change_type: ChangeTypeCounts {
                    modified: 1,
                    ..Default::default()
                },
                by_impact: ImpactCounts {
                    critical: 1,
                    ..Default::default()
                },
            },
        };

        let impact = ChangeImpactSummary::from_change_set(&change_set);

        assert_eq!(impact.highest_impact, ChangeImpact::Critical);
        assert!(impact.has_critical);
        assert!(impact.has_security_related);
    }

    #[test]
    fn test_worker_metadata() {
        let metadata = WorkerMetadata::new("test-worker");
        assert_eq!(metadata.worker_id, "test-worker");
        assert_eq!(metadata.provider, "gix");
        assert_eq!(metadata.engine, "gitoxide");
    }

    #[test]
    fn test_worker_registration() {
        let registration = WorkerRegistration::new("test-worker");
        assert_eq!(registration.worker_id, "test-worker");
        assert_eq!(registration.capabilities.len(), 2);
        assert_eq!(registration.capabilities[0].name, "git.diff");
        assert_eq!(registration.capabilities[0].version, "1");
    }

    #[test]
    fn test_health_response_with_state() {
        let response = HealthResponse::with_state(100, 5);
        assert_eq!(response.status, "ok");
        assert_eq!(response.jobs_processed, 100);
        assert_eq!(response.cache_repositories, 5);
    }
}
