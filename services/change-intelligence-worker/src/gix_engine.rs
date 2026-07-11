//! Git analysis engine using gix for computing diffs and change intelligence.

use std::path::Path;

use anyhow::{Context, Result};
use gix::object::tree::diff::Change;
use tracing::{debug, info, instrument};

use crate::models::{
    ChangeAnalysisJob, ChangeImpact, ChangeSet, ChangeSummary, ChangeType, ChangeTypeCounts,
    FileChange, ImpactCounts,
};

/// Analyze changes between two commits in a repository.
#[instrument(skip(job, cache_path), fields(job_id = %job.job_id, repo = %job.repository_url))]
pub async fn analyze(job: &ChangeAnalysisJob, cache_path: &Path) -> Result<ChangeSet> {
    info!("Starting change analysis");

    // Derive a cache directory name from the repository URL
    let repo_cache_dir = get_repo_cache_dir(&job.repository_url, cache_path);

    // Clone or fetch the repository
    let repo = prepare_repository(&job.repository_url, &repo_cache_dir).await?;

    // Resolve the commit references
    let base_oid = resolve_commit(&repo, &job.base_commit)?;
    let head_oid = resolve_commit(&repo, &job.head_commit)?;

    debug!(?base_oid, ?head_oid, "Resolved commits");

    // Compute the diff between commits
    let files = compute_diff(&repo, base_oid, head_oid)?;

    // Build the summary
    let summary = build_summary(&files);

    info!(
        files_changed = files.len(),
        additions = summary.total_additions,
        deletions = summary.total_deletions,
        "Analysis complete"
    );

    Ok(ChangeSet {
        base_commit: base_oid.to_string(),
        head_commit: head_oid.to_string(),
        files,
        summary,
    })
}

/// Derive a safe directory name from a repository URL.
fn get_repo_cache_dir(repo_url: &str, cache_path: &Path) -> std::path::PathBuf {
    // Create a safe directory name from the URL
    let safe_name: String = repo_url
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();

    cache_path.join(&safe_name)
}

/// Prepare a repository by cloning or fetching updates.
#[instrument(skip_all, fields(repo_url = repo_url, cache_dir = ?repo_cache_dir))]
async fn prepare_repository(
    repo_url: &str,
    repo_cache_dir: &Path,
) -> Result<gix::Repository> {
    if repo_cache_dir.exists() {
        debug!("Repository cache exists, opening");
        let repo = gix::open(repo_cache_dir).context("Failed to open cached repository")?;

        // Fetch latest changes
        debug!("Fetching latest changes");
        fetch_repository(&repo)?;

        Ok(repo)
    } else {
        info!("Cloning repository to cache");
        clone_repository(repo_url, repo_cache_dir)
    }
}

/// Clone a repository to the specified path.
fn clone_repository(repo_url: &str, dest: &Path) -> Result<gix::Repository> {
    // Create parent directories if needed
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).context("Failed to create cache directory")?;
    }

    // Use gix to prepare a clone
    let url = gix::url::parse(repo_url.into()).context("Failed to parse repository URL")?;

    // Prepare a bare clone
    let mut prep = gix::prepare_clone_bare(url, dest).context("Failed to prepare clone")?;

    let (repo, _) = prep
        .fetch_only(gix::progress::Discard, &std::sync::atomic::AtomicBool::new(false))
        .context("Failed to clone repository")?;

    Ok(repo)
}

/// Fetch updates for an existing repository.
fn fetch_repository(repo: &gix::Repository) -> Result<()> {
    // Get the default remote (usually "origin")
    let remote = repo
        .find_default_remote(gix::remote::Direction::Fetch)
        .context("No default remote configured")?
        .context("Failed to find remote")?;

    // Perform fetch
    let _outcome = remote
        .connect(gix::remote::Direction::Fetch)
        .context("Failed to connect to remote")?
        .prepare_fetch(gix::progress::Discard, Default::default())
        .context("Failed to prepare fetch")?
        .receive(gix::progress::Discard, &std::sync::atomic::AtomicBool::new(false))
        .context("Failed to receive objects")?;

    debug!("Fetch complete");
    Ok(())
}

/// Resolve a commit reference (SHA, branch, tag) to an object ID.
fn resolve_commit(repo: &gix::Repository, reference: &str) -> Result<gix::ObjectId> {
    use bstr::ByteSlice;

    // Try to parse as a direct SHA first
    if let Ok(oid) = gix::ObjectId::from_hex(reference.as_bytes()) {
        return Ok(oid);
    }

    // Try to resolve as a reference
    let resolved = repo
        .rev_parse_single(reference.as_bytes().as_bstr())
        .with_context(|| format!("Failed to resolve reference: {reference}"))?;

    Ok(resolved.detach())
}

/// Compute the diff between two commits.
fn compute_diff(
    repo: &gix::Repository,
    base_oid: gix::ObjectId,
    head_oid: gix::ObjectId,
) -> Result<Vec<FileChange>> {
    let base_commit = repo
        .find_object(base_oid)
        .context("Failed to find base commit")?
        .peel_to_commit()
        .context("Base reference is not a commit")?;

    let head_commit = repo
        .find_object(head_oid)
        .context("Failed to find head commit")?
        .peel_to_commit()
        .context("Head reference is not a commit")?;

    let base_tree = base_commit.tree().context("Failed to get base tree")?;
    let head_tree = head_commit.tree().context("Failed to get head tree")?;

    // Collect changes between trees
    let mut files = Vec::new();

    base_tree
        .changes()
        .context("Failed to create diff platform")?
        .for_each_to_obtain_tree(&head_tree, |change| {
            if let Some(file_change) = convert_change(&change) {
                files.push(file_change);
            }
            Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
        })
        .context("Failed to compute tree diff")?;

    Ok(files)
}

/// Convert a gix tree change to our FileChange model.
fn convert_change(change: &Change<'_, '_, '_>) -> Option<FileChange> {
    let (path, change_type) = match change {
        Change::Addition { location, .. } => {
            (location.to_string(), ChangeType::Added)
        }
        Change::Deletion { location, .. } => {
            (location.to_string(), ChangeType::Deleted)
        }
        Change::Modification { location, .. } => {
            (location.to_string(), ChangeType::Modified)
        }
        Change::Rewrite { source_location, copy, .. } => {
            let ct = if *copy { ChangeType::Copied } else { ChangeType::Renamed };
            (source_location.to_string(), ct)
        }
    };

    let impact = classify_impact(&path);

    Some(FileChange {
        path,
        change_type,
        additions: 0, // Line counts would require blob diff
        deletions: 0,
        impact,
    })
}

/// Classify the impact of a change based on file path.
#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn classify_impact(path: &str) -> ChangeImpact {
    // Path is already lowercased, so extension comparisons are effectively case-insensitive
    let path_lower = path.to_lowercase();

    // Critical: security, authentication, authorization
    if path_lower.contains("security")
        || path_lower.contains("auth")
        || path_lower.contains("secret")
        || path_lower.contains("crypt")
    {
        return ChangeImpact::Critical;
    }

    // High: core source files, API definitions
    if path_lower.ends_with(".rs")
        || path_lower.ends_with(".go")
        || path_lower.ends_with(".py")
        || path_lower.ends_with(".js")
        || path_lower.ends_with(".ts")
        || path_lower.contains("/api/")
        || path_lower.contains("/core/")
    {
        // But tests are medium impact
        if path_lower.contains("test") || path_lower.contains("spec") {
            return ChangeImpact::Medium;
        }
        return ChangeImpact::High;
    }

    // Medium: configuration, dependencies
    if path_lower.ends_with(".toml")
        || path_lower.ends_with(".yaml")
        || path_lower.ends_with(".yml")
        || path_lower.ends_with(".json")
        || path_lower.contains("config")
    {
        return ChangeImpact::Medium;
    }

    // Low: documentation, comments
    if path_lower.ends_with(".md")
        || path_lower.ends_with(".txt")
        || path_lower.ends_with(".rst")
        || path_lower.contains("doc")
        || path_lower.contains("readme")
    {
        return ChangeImpact::Low;
    }

    // Default to medium for unknown file types
    ChangeImpact::Medium
}

/// Build a summary from a list of file changes.
fn build_summary(files: &[FileChange]) -> ChangeSummary {
    let mut by_change_type = ChangeTypeCounts::default();
    let mut by_impact = ImpactCounts::default();
    let mut total_additions = 0u32;
    let mut total_deletions = 0u32;

    for file in files {
        // Count by change type
        match file.change_type {
            ChangeType::Added => by_change_type.added += 1,
            ChangeType::Modified => by_change_type.modified += 1,
            ChangeType::Deleted => by_change_type.deleted += 1,
            ChangeType::Renamed => by_change_type.renamed += 1,
            ChangeType::Copied => by_change_type.copied += 1,
        }

        // Count by impact
        match file.impact {
            ChangeImpact::Low => by_impact.low += 1,
            ChangeImpact::Medium => by_impact.medium += 1,
            ChangeImpact::High => by_impact.high += 1,
            ChangeImpact::Critical => by_impact.critical += 1,
        }

        total_additions += file.additions;
        total_deletions += file.deletions;
    }

    ChangeSummary {
        files_changed: files.len() as u32,
        total_additions,
        total_deletions,
        by_change_type,
        by_impact,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_impact_critical() {
        assert_eq!(classify_impact("src/security/auth.rs"), ChangeImpact::Critical);
        assert_eq!(classify_impact("lib/crypto.py"), ChangeImpact::Critical);
    }

    #[test]
    fn test_classify_impact_high() {
        assert_eq!(classify_impact("src/main.rs"), ChangeImpact::High);
        assert_eq!(classify_impact("api/handlers.go"), ChangeImpact::High);
    }

    #[test]
    fn test_classify_impact_medium() {
        assert_eq!(classify_impact("tests/unit_test.rs"), ChangeImpact::Medium);
        assert_eq!(classify_impact("Cargo.toml"), ChangeImpact::Medium);
        assert_eq!(classify_impact("config.yaml"), ChangeImpact::Medium);
    }

    #[test]
    fn test_classify_impact_low() {
        assert_eq!(classify_impact("README.md"), ChangeImpact::Low);
        assert_eq!(classify_impact("docs/guide.txt"), ChangeImpact::Low);
    }

    #[test]
    fn test_get_repo_cache_dir() {
        let cache_path = Path::new("/data/repos");
        let result = get_repo_cache_dir("https://github.com/user/repo.git", cache_path);
        assert!(result.to_string_lossy().contains("github_com_user_repo_git"));
    }
}
