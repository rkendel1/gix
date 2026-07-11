//! Repository cache management for efficient repository handling.
//!
//! This module provides a cache layer for Git repositories, allowing the worker
//! to reuse cloned repositories across multiple jobs instead of cloning each time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::{debug, info, instrument};

/// Trait for repository cache implementations.
#[allow(dead_code)]
#[async_trait]
pub trait RepositoryCache: Send + Sync {
    /// Ensure a repository is available in the cache.
    /// Returns the path to the cached repository.
    async fn ensure_repository(&self, url: &str) -> Result<PathBuf>;

    /// Checkout a specific commit in the cached repository.
    /// Note: For bare repositories, this doesn't actually checkout files,
    /// it just ensures the commit is available for analysis.
    async fn checkout(&self, repo: &Path, commit: &str) -> Result<()>;

    /// Get the number of repositories currently cached.
    fn cached_count(&self) -> u64;
}

/// File-system based repository cache.
#[derive(Debug)]
pub struct FileSystemCache {
    /// Base directory for the cache.
    base_path: PathBuf,
    /// Counter for cached repositories.
    cached_repos: AtomicU64,
}

impl FileSystemCache {
    /// Create a new file system cache at the given path.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        let base_path = base_path.into();

        // Count existing cached repositories
        let cached_repos = count_cached_repositories(&base_path);

        Self {
            base_path,
            cached_repos: AtomicU64::new(cached_repos),
        }
    }

    /// Get the cache directory for a repository URL.
    fn get_repo_cache_dir(&self, repo_url: &str) -> PathBuf {
        // Create a safe directory name from the URL
        let safe_name: String = repo_url
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();

        self.base_path.join(&safe_name)
    }
}

#[async_trait]
impl RepositoryCache for FileSystemCache {
    #[instrument(skip(self), fields(cache_path = ?self.base_path))]
    async fn ensure_repository(&self, url: &str) -> Result<PathBuf> {
        let repo_cache_dir = self.get_repo_cache_dir(url);

        if repo_cache_dir.exists() {
            debug!("Repository cache exists, opening and fetching");
            let repo =
                gix::open(&repo_cache_dir).context("Failed to open cached repository")?;

            // Fetch latest changes
            fetch_repository(&repo)?;
        } else {
            info!("Cloning repository to cache");
            clone_repository(url, &repo_cache_dir)?;
            self.cached_repos.fetch_add(1, Ordering::SeqCst);
        }

        Ok(repo_cache_dir)
    }

    #[instrument(skip(self))]
    async fn checkout(&self, repo_path: &Path, commit: &str) -> Result<()> {
        let repo = gix::open(repo_path).context("Failed to open repository")?;

        // Verify the commit exists
        use bstr::ByteSlice;
        let _oid = if let Ok(oid) = gix::ObjectId::from_hex(commit.as_bytes()) {
            // Verify it exists in the repository
            repo.find_object(oid)
                .with_context(|| format!("Commit {commit} not found in repository"))?;
            oid
        } else {
            // Try to resolve as a reference
            repo.rev_parse_single(commit.as_bytes().as_bstr())
                .with_context(|| format!("Failed to resolve reference: {commit}"))?
                .detach()
        };

        debug!(commit = %commit, "Commit verified");
        Ok(())
    }

    fn cached_count(&self) -> u64 {
        self.cached_repos.load(Ordering::SeqCst)
    }
}

/// Clone a repository to the specified path.
#[allow(dead_code)]
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
        .fetch_only(
            gix::progress::Discard,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .context("Failed to clone repository")?;

    Ok(repo)
}

/// Fetch updates for an existing repository.
#[allow(dead_code)]
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
        .receive(
            gix::progress::Discard,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .context("Failed to receive objects")?;

    debug!("Fetch complete");
    Ok(())
}

/// Count the number of existing cached repositories.
fn count_cached_repositories(base_path: &Path) -> u64 {
    if !base_path.exists() {
        return 0;
    }

    std::fs::read_dir(base_path).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .count() as u64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_repo_cache_dir() {
        let cache = FileSystemCache::new("/data/repos");
        let result = cache.get_repo_cache_dir("https://github.com/user/repo.git");
        assert!(result
            .to_string_lossy()
            .contains("github_com_user_repo_git"));
    }

    #[test]
    fn test_cache_dir_is_deterministic() {
        let cache = FileSystemCache::new("/data/repos");
        let dir1 = cache.get_repo_cache_dir("https://github.com/user/repo.git");
        let dir2 = cache.get_repo_cache_dir("https://github.com/user/repo.git");
        assert_eq!(dir1, dir2);
    }

    #[test]
    fn test_different_urls_get_different_dirs() {
        let cache = FileSystemCache::new("/data/repos");
        let dir1 = cache.get_repo_cache_dir("https://github.com/user/repo1.git");
        let dir2 = cache.get_repo_cache_dir("https://github.com/user/repo2.git");
        assert_ne!(dir1, dir2);
    }
}
