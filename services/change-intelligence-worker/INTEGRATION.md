# Change Intelligence Worker - Integration Guide & API Contract

This document provides a complete integration guide for coordinators and consumers of the Change Intelligence Worker service.

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [API Contract](#api-contract)
  - [Worker Registration](#worker-registration)
  - [Job Claiming](#job-claiming)
  - [Job Completion](#job-completion)
  - [Health Check](#health-check)
- [Data Models](#data-models)
- [Fingerprinting & Change Tracking](#fingerprinting--change-tracking)
- [Impact Classification](#impact-classification)
- [Error Handling](#error-handling)
- [Examples](#examples)

---

## Overview

The Change Intelligence Worker is a stateless service that analyzes Git repository changes using the [gix](https://github.com/GitoxideLabs/gitoxide) library. It computes diffs between commits and returns structured change intelligence with impact classification.

**Key Features:**
- Commit-to-commit diff analysis
- File change detection (added, modified, deleted, renamed, copied)
- Impact classification (critical, high, medium, low)
- Deterministic artifact hashing for change tracking
- Repository caching for efficient repeated analysis

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        COORDINATOR                               │
│  (Maintains job queue, dispatches work, stores results)         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ HTTP/JSON
                              │
         ┌────────────────────┼────────────────────┐
         │                    │                    │
         ▼                    ▼                    ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│  gix-worker-01  │  │  gix-worker-02  │  │  gix-worker-03  │
│  (Instance 1)   │  │  (Instance 2)   │  │  (Instance 3)   │
└─────────────────┘  └─────────────────┘  └─────────────────┘
         │                    │                    │
         ▼                    ▼                    ▼
    ┌─────────┐          ┌─────────┐          ┌─────────┐
    │ Repo    │          │ Repo    │          │ Repo    │
    │ Cache   │          │ Cache   │          │ Cache   │
    └─────────┘          └─────────┘          └─────────┘
```

### Communication Flow

1. **Worker Registration**: Worker starts and registers capabilities with coordinator
2. **Job Polling**: Worker polls for available jobs matching its capabilities
3. **Job Processing**: Worker clones/fetches repo, computes diff, classifies changes
4. **Result Submission**: Worker submits structured results back to coordinator

---

## API Contract

### Schema Version

All responses include a `schema_version` field. The current version is **`v1`**.

Breaking changes will increment the version. Coordinators should validate this field.

---

### Worker Registration

Workers must register with the coordinator on startup.

**Coordinator Endpoint (expected):**
```
POST /api/workers/register
```

**Request Body:**
```json
{
  "worker_id": "gix-worker-01",
  "capabilities": [
    {
      "name": "git.diff",
      "version": "1"
    },
    {
      "name": "change.intelligence",
      "version": "1"
    }
  ],
  "version": "0.1.0"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `worker_id` | string | Unique identifier for this worker instance |
| `capabilities` | array | List of versioned capabilities this worker provides |
| `capabilities[].name` | string | Capability name (e.g., `git.diff`, `change.intelligence`) |
| `capabilities[].version` | string | Version of this capability |
| `version` | string | Worker software version |

**Expected Response:** `200 OK` or `201 Created`

---

### Job Claiming

Workers poll for jobs matching their capabilities.

**Coordinator Endpoint (expected):**
```
POST /api/jobs/claim?worker_id={worker_id}&capability={capability}
```

**Query Parameters:**
| Parameter | Required | Description |
|-----------|----------|-------------|
| `worker_id` | Yes | The worker's unique identifier |
| `capability` | Yes | Capability to claim jobs for (e.g., `change.intelligence`) |

**Response (Job Available) - `200 OK`:**
```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "repository_url": "https://github.com/org/repo.git",
  "base_commit": "abc123def456789012345678901234567890abcd",
  "head_commit": "def456abc789012345678901234567890abcdef12",
  "paths": []
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `job_id` | UUID | Yes | Unique identifier for this job |
| `repository_url` | string | Yes | Git repository URL (HTTPS or SSH) |
| `base_commit` | string | Yes | Base commit SHA (40 hex characters) or reference |
| `head_commit` | string | Yes | Head commit SHA (40 hex characters) or reference |
| `paths` | array | No | Optional list of paths to filter analysis (not yet implemented) |

**Response (No Job Available) - `204 No Content`:**
Empty body

---

### Job Completion

Workers submit analysis results to the coordinator.

**Coordinator Endpoint (expected):**
```
POST /api/jobs/complete
```

**Request Body (Success):**
```json
{
  "schema_version": "v1",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "success": true,
  "error": null,
  "worker": {
    "worker_id": "gix-worker-01",
    "provider": "gix",
    "engine": "gitoxide",
    "version": "0.1.0"
  },
  "repository": {
    "repository_url": "https://github.com/org/repo.git",
    "base_commit": "abc123def456789012345678901234567890abcd",
    "target_commit": "def456abc789012345678901234567890abcdef12",
    "tree_hash": "1234567890abcdef1234567890abcdef12345678"
  },
  "change_set": {
    "base_commit": "abc123def456789012345678901234567890abcd",
    "head_commit": "def456abc789012345678901234567890abcdef12",
    "files": [
      {
        "path": "src/main.rs",
        "change_type": "modified",
        "additions": 0,
        "deletions": 0,
        "impact": "high"
      },
      {
        "path": "README.md",
        "change_type": "modified",
        "additions": 0,
        "deletions": 0,
        "impact": "low"
      }
    ],
    "summary": {
      "files_changed": 2,
      "total_additions": 0,
      "total_deletions": 0,
      "by_change_type": {
        "added": 0,
        "modified": 2,
        "deleted": 0,
        "renamed": 0,
        "copied": 0
      },
      "by_impact": {
        "low": 1,
        "medium": 0,
        "high": 1,
        "critical": 0
      }
    }
  },
  "impact": {
    "highest_impact": "high",
    "has_critical": false,
    "has_security_related": false,
    "distribution": {
      "low": 1,
      "medium": 0,
      "high": 1,
      "critical": 0
    }
  },
  "artifact_hash": "sha256:abc123def456..."
}
```

**Request Body (Failure):**
```json
{
  "schema_version": "v1",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "success": false,
  "error": "Failed to clone repository: authentication required",
  "worker": {
    "worker_id": "gix-worker-01",
    "provider": "gix",
    "engine": "gitoxide",
    "version": "0.1.0"
  },
  "repository": null,
  "change_set": null,
  "impact": null,
  "artifact_hash": null
}
```

**Expected Response:** `200 OK`

---

### Health Check

Workers expose a health check endpoint for monitoring.

**Worker Endpoint:**
```
GET /health
```

**Response:**
```json
{
  "status": "ok",
  "capability": "change-intelligence",
  "provider": "gix",
  "version": "0.1.0",
  "jobs_processed": 42,
  "cache_repositories": 5
}
```

| Field | Type | Description |
|-------|------|-------------|
| `status` | string | Worker health status (`ok`, `degraded`, `error`) |
| `capability` | string | Primary capability of this worker |
| `provider` | string | Git implementation provider |
| `version` | string | Worker software version |
| `jobs_processed` | number | Total jobs successfully completed since startup |
| `cache_repositories` | number | Number of repositories currently cached |

---

## Data Models

### Change Types

| Value | Description |
|-------|-------------|
| `added` | New file was added |
| `modified` | Existing file was modified |
| `deleted` | File was deleted |
| `renamed` | File was renamed (possibly with modifications) |
| `copied` | File was copied from another file |

### Impact Levels

| Value | Description | Examples |
|-------|-------------|----------|
| `critical` | Security-sensitive changes | Files containing: `security`, `auth`, `secret`, `crypt` |
| `high` | Core logic changes | `.rs`, `.go`, `.py`, `.js`, `.ts` files; `/api/`, `/core/` directories |
| `medium` | Configuration/test changes | Tests, `.toml`, `.yaml`, `.yml`, `.json` files, config directories |
| `low` | Documentation changes | `.md`, `.txt`, `.rst` files; doc directories, README files |

---

## Fingerprinting & Change Tracking

### Artifact Hash

Every successful analysis produces a deterministic `artifact_hash` that uniquely identifies the change set. This enables:

1. **Deduplication**: Detect if the same changes have been analyzed before
2. **Verification**: Confirm that two workers produce identical results for the same input
3. **Change Tracking**: Track whether a fingerprinted state has changed

**Hash Format:**
```
sha256:<64-character-hex-digest>
```

**Hash Computation:**
1. File changes are sorted alphabetically by path
2. The sorted change set is serialized to canonical JSON
3. SHA-256 is computed over the JSON bytes

**Properties:**
- **Deterministic**: Same input always produces same hash
- **Order-independent**: File order in input doesn't affect hash
- **Content-sensitive**: Any change to the change set produces a different hash

### Tree Hash

The `repository.tree_hash` field contains the Git tree hash of the head commit. This provides:

1. **State Verification**: Confirm the exact repository state that was analyzed
2. **Reproducibility**: Re-analyze the same tree hash to verify results

### Using Fingerprints for Change Detection

To detect if a previously fingerprinted state has changed:

```
1. Store the original artifact_hash after first analysis
2. When checking for changes:
   a. Request a new analysis with same base_commit and new head_commit
   b. Compare new artifact_hash with stored hash
   c. If different, changes have occurred since fingerprinting
```

**Example Flow:**

```
Initial State:
  base: commit_A
  head: commit_B
  artifact_hash: sha256:abc123...

Later Check:
  base: commit_A  (same baseline)
  head: commit_C  (new head)
  artifact_hash: sha256:def456...  (different = changes detected)
```

---

## Impact Classification

Files are classified by impact level using path-based heuristics:

### Classification Rules (in priority order)

1. **Critical** - Security-sensitive patterns:
   - Path contains: `security`, `auth`, `secret`, `crypt`

2. **High** - Core source files:
   - Extensions: `.rs`, `.go`, `.py`, `.js`, `.ts`
   - Path contains: `/api/`, `/core/`
   - Exception: Test files → Medium

3. **Medium** - Configuration and tests:
   - Test files (path contains `test` or `spec`)
   - Extensions: `.toml`, `.yaml`, `.yml`, `.json`
   - Path contains: `config`

4. **Low** - Documentation:
   - Extensions: `.md`, `.txt`, `.rst`
   - Path contains: `doc`, `readme`

5. **Default** - Unknown file types → Medium

### Impact Summary

The `impact` field in the response provides:

```json
{
  "highest_impact": "high",      // Highest level in change set
  "has_critical": false,         // Quick check for critical changes
  "has_security_related": true,  // Files matching security patterns
  "distribution": {              // Count by level
    "low": 5,
    "medium": 10,
    "high": 3,
    "critical": 0
  }
}
```

---

## Error Handling

### Common Errors

| Error | Cause | Resolution |
|-------|-------|------------|
| `Failed to clone repository` | Invalid URL or auth required | Check repository URL and access permissions |
| `Failed to resolve reference: <ref>` | Commit/branch doesn't exist | Verify base_commit and head_commit exist |
| `Failed to find <commit>` | Commit not in repository | Ensure commits are pushed to remote |
| `No default remote configured` | Repository has no origin | Configure remote for cached repository |

### Error Response Format

```json
{
  "schema_version": "v1",
  "job_id": "...",
  "success": false,
  "error": "Descriptive error message",
  "worker": { ... },
  "repository": null,
  "change_set": null,
  "impact": null,
  "artifact_hash": null
}
```

---

## Examples

### Example 1: Analyze a Pull Request

**Job Request:**
```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "repository_url": "https://github.com/myorg/myrepo.git",
  "base_commit": "main",
  "head_commit": "feature/new-api",
  "paths": []
}
```

**Successful Response:**
```json
{
  "schema_version": "v1",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "success": true,
  "worker": {
    "worker_id": "gix-worker-01",
    "provider": "gix",
    "engine": "gitoxide",
    "version": "0.1.0"
  },
  "repository": {
    "repository_url": "https://github.com/myorg/myrepo.git",
    "base_commit": "abc123...",
    "target_commit": "def456...",
    "tree_hash": "789abc..."
  },
  "change_set": {
    "base_commit": "abc123...",
    "head_commit": "def456...",
    "files": [
      {
        "path": "src/api/handlers.rs",
        "change_type": "added",
        "additions": 0,
        "deletions": 0,
        "impact": "high"
      },
      {
        "path": "src/api/mod.rs",
        "change_type": "modified",
        "additions": 0,
        "deletions": 0,
        "impact": "high"
      },
      {
        "path": "tests/api_test.rs",
        "change_type": "added",
        "additions": 0,
        "deletions": 0,
        "impact": "medium"
      }
    ],
    "summary": {
      "files_changed": 3,
      "total_additions": 0,
      "total_deletions": 0,
      "by_change_type": {
        "added": 2,
        "modified": 1,
        "deleted": 0,
        "renamed": 0,
        "copied": 0
      },
      "by_impact": {
        "low": 0,
        "medium": 1,
        "high": 2,
        "critical": 0
      }
    }
  },
  "impact": {
    "highest_impact": "high",
    "has_critical": false,
    "has_security_related": false,
    "distribution": {
      "low": 0,
      "medium": 1,
      "high": 2,
      "critical": 0
    }
  },
  "artifact_hash": "sha256:a1b2c3d4e5f6..."
}
```

### Example 2: Coordinator Implementation (Pseudocode)

```python
class ChangeIntelligenceCoordinator:
    def __init__(self):
        self.workers = {}
        self.job_queue = []
        self.results = {}
    
    # Worker registration endpoint
    def register_worker(self, registration):
        self.workers[registration.worker_id] = {
            "capabilities": registration.capabilities,
            "version": registration.version,
            "last_seen": now()
        }
        return {"status": "registered"}
    
    # Job claiming endpoint
    def claim_job(self, worker_id, capability):
        for job in self.job_queue:
            if job.capability == capability and not job.claimed:
                job.claimed = True
                job.worker_id = worker_id
                job.claimed_at = now()
                return job
        return None  # 204 No Content
    
    # Job completion endpoint
    def complete_job(self, result):
        self.results[result.job_id] = {
            "success": result.success,
            "error": result.error,
            "artifact_hash": result.artifact_hash,
            "change_set": result.change_set,
            "impact": result.impact,
            "completed_at": now()
        }
        
        # Store artifact_hash for change tracking
        if result.success:
            self.store_fingerprint(
                result.repository.repository_url,
                result.repository.target_commit,
                result.artifact_hash
            )
        
        return {"status": "completed"}
    
    # Check if changes occurred since fingerprint
    def has_changes_since(self, repo_url, original_commit, new_commit):
        original_hash = self.get_fingerprint(repo_url, original_commit)
        
        # Request new analysis
        job_id = self.enqueue_job(repo_url, original_commit, new_commit)
        result = self.wait_for_result(job_id)
        
        if not result.success:
            raise AnalysisError(result.error)
        
        return result.artifact_hash != original_hash
```

---

## Coordinator Requirements

To integrate with the Change Intelligence Worker, coordinators must implement:

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/workers/register` | POST | Accept worker registration |
| `/api/jobs/claim` | POST | Return available jobs or 204 |
| `/api/jobs/complete` | POST | Accept job results |

### Recommended Coordinator Features

1. **Job Queue Management**
   - Maintain a queue of pending analysis jobs
   - Track job state (pending, claimed, completed, failed)
   - Implement job timeouts for stuck workers

2. **Worker Health Monitoring**
   - Track worker last-seen timestamps
   - Periodically check worker health endpoints
   - Remove stale workers from the pool

3. **Result Storage**
   - Store completed analysis results
   - Index by job_id, repository, and artifact_hash
   - Implement result expiration policies

4. **Fingerprint Tracking**
   - Store artifact_hash values for change detection
   - Associate fingerprints with repository + commit pairs
   - Provide APIs for fingerprint comparison

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1 | 2024 | Initial release |

---

## Support

For issues or questions, please open an issue in the repository or contact the maintainers.
