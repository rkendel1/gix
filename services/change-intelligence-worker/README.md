# Change Intelligence Worker

A standalone worker service for Git repository diff analysis using [gix](https://github.com/GitoxideLabs/gitoxide).

## Overview

This worker runs independently from the main TryThisSoftware API and communicates through an execution coordination model. It:

- ✅ Claims jobs from a coordinator
- ✅ Clones/fetches repositories
- ✅ Resolves commits
- ✅ Computes diffs using gix
- ✅ Classifies changes by impact
- ✅ Returns structured change intelligence

It does **not**:

- ❌ Store users
- ❌ Store repositories persistently
- ❌ Issue launch contracts
- ❌ Run untrusted applications
- ❌ Own execution policy

## Architecture

```
         Public
TryThisSoftware API
      Coordinator
          |
          |
    Work Queue / RPC
          |
          v
change-intelligence-worker
          |
          v
        gix
          |
          v
  ChangeSet + Impact
          |
          v
  Coordinator persistence
```

## Configuration

Environment variables:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `COORDINATOR_URL` | Yes | - | URL of the coordinator service |
| `WORKER_ID` | No | `gix-worker` | Unique identifier for this worker |
| `REPOSITORY_CACHE` | No | `/data/repos` | Path to repository cache |
| `HEALTH_PORT` | No | `8080` | Port for health check endpoint |
| `POLL_INTERVAL_SECS` | No | `2` | Job polling interval in seconds |

## Building

```bash
# From the repository root
cargo build --release --package change-intelligence-worker

# Or from this directory
cd services/change-intelligence-worker
cargo build --release
```

## Running Locally

```bash
export COORDINATOR_URL=http://localhost:3000
export WORKER_ID=gix-worker-local
export REPOSITORY_CACHE=/tmp/repos

cargo run --release --package change-intelligence-worker
```

## Docker

Build:

```bash
# From repository root
docker build -f services/change-intelligence-worker/Dockerfile -t change-intelligence-worker .
```

Run:

```bash
docker run -e COORDINATOR_URL=https://api.example.com change-intelligence-worker
```

## Deployment to Fly.io

### Initial Setup

```bash
cd services/change-intelligence-worker

# Create the app (first time only)
fly launch --no-deploy

# Create the volume for repository caching
fly volumes create repo_cache --size 10 --region iad
```

### Deploy

```bash
fly deploy
```

## API Endpoints

### Health Check

```
GET /health
```

Response:

```json
{
  "status": "ok",
  "capability": "change-intelligence",
  "provider": "gix"
}
```

## Worker Registration

On startup, the worker registers with the coordinator:

```
POST /api/workers/register
{
  "worker_id": "gix-worker-01",
  "capabilities": [
    "git.diff",
    "change.intelligence"
  ],
  "version": "0.1.0"
}
```

## Job Protocol

### Claiming Jobs

```
POST /api/jobs/claim?worker_id=gix-worker-01&capability=change.intelligence
```

Response (job available):

```json
{
  "job_id": "uuid",
  "repository_url": "https://github.com/org/repo.git",
  "base_commit": "abc123",
  "head_commit": "def456",
  "paths": []
}
```

Response (no job): `204 No Content`

### Completing Jobs

```
POST /api/jobs/complete
{
  "job_id": "uuid",
  "success": true,
  "change_set": {
    "base_commit": "abc123...",
    "head_commit": "def456...",
    "files": [...],
    "summary": {...}
  }
}
```

## Scaling

Initially run with a single worker. Scale horizontally by deploying multiple instances:

```
          Coordinator
              |
   -----------------------
   |          |          |
gix-1      gix-2      gix-3
```

Jobs are distributed across available workers.

## License

MIT OR Apache-2.0
