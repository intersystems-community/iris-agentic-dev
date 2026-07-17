---
name: iris-container-graceful-shutdown
description: >
  IRIS data persists across container restarts ONLY if IRIS is stopped gracefully
  before docker stop. Without this, docker stop sends SIGKILL after grace period,
  leaving the WIJ (write image journal) dirty. Data recovers on next start but
  takes 30-300s. Load when: stopping IRIS containers, setting up docker-compose
  for production, or debugging "tables exist but rows are gone after restart".
tags: [iris, docker, data-loss, shutdown, wij, iris-devtester, container]
---

# IRIS Container Graceful Shutdown

## The Problem

`docker stop` sends SIGTERM then SIGKILL after the grace period. **IRIS does not trap SIGTERM in its default entrypoint.** Docker kills it. The WIJ (write image journal — IRIS's write buffer) is not flushed. On restart, IRIS runs journal recovery which takes 30-300 seconds. Uncommitted in-flight writes are lost.

**Symptom**: After `docker stop` + `docker start`, tables exist (schema survived) but rows are 0.

**This is silent** — no error, no warning, IRIS starts cleanly, data appears empty.

---

## Fix: Always Run `iris stop IRIS quietly` Before `docker stop`

```bash
docker exec <container> iris stop IRIS quietly
docker stop <container>
```

This flushes the WIJ, checkpoints globals, and marks the database clean. Next start is instant.

---

## docker-compose Pattern

```yaml
services:
  iris:
    image: intersystemsdc/iris-community:latest
    stop_grace_period: 60s    # Give docker stop 60s before SIGKILL
    # Note: stop_grace_period alone is NOT enough — IRIS doesn't trap SIGTERM.
    # You must also run 'iris stop IRIS quietly' before 'docker compose down'.
```

**Shutdown script** (create as `scripts/stop-iris.sh`):
```bash
#!/bin/bash
CONTAINER="${1:-iris}"
docker exec "$CONTAINER" iris stop IRIS quietly
docker stop "$CONTAINER"
```

---

## iris-devtester (v1.18.1+)

`IRISContainer.__exit__()` now calls `stop_gracefully()` automatically:

```python
with IRISContainer.community() as iris:
    conn = iris.get_connection()
    # ... do work ...
# __exit__ calls iris stop IRIS quietly before docker stop
```

`stop_gracefully()` is also callable directly:
```python
iris.stop_gracefully()   # Returns True on success, False if container not running
docker stop container    # Safe to call after
```

---

## Why a Webgateway, CPF Merge, or stop_grace_period Alone Is Not Enough

| Approach | Does it prevent data loss? | Why |
|---|---|---|
| `stop_grace_period: 60s` | ❌ | IRIS doesn't trap SIGTERM by default |
| `stop_signal: SIGUSR1` | ❌ | IRIS ignores SIGUSR1/SIGUSR2 |
| `iris stop IRIS quietly` via exec | ✅ | Explicit graceful shutdown with WIJ flush |
| Custom entrypoint that traps SIGTERM | ✅ | Intercepts SIGTERM → runs iris stop → exits |

---

## Custom Entrypoint (Full SIGTERM Trap)

For docker-compose without manual pre-stop:

```bash
#!/bin/sh
# graceful-iris-entrypoint.sh
/iris-main &
IRIS_PID=$!
trap 'iris stop IRIS quietly; wait $IRIS_PID' SIGTERM INT TERM
wait $IRIS_PID
```

```yaml
services:
  iris:
    entrypoint: ["/bin/sh", "/graceful-iris-entrypoint.sh"]
    stop_grace_period: 60s
    volumes:
      - ./graceful-iris-entrypoint.sh:/graceful-iris-entrypoint.sh:ro
```

Note: This conflicts with the standard IRIS `-b`/`-a` hook mechanism. Use only if you don't need pre/post hooks.

---

## Kubernetes PreStop Hook

```yaml
lifecycle:
  preStop:
    exec:
      command: ["iris", "stop", "IRIS", "quietly"]
terminationGracePeriodSeconds: 60
```

---

## Diagnosis

```bash
# After restart, check WIJ state at startup
docker logs <container> 2>&1 | grep -i "wij\|journal\|recover\|dirty\|clean"

# Clean startup:   "Journal startup: WIJ is clean"
# Recovery needed: "Journal startup: WIJ recovery in progress"
```

Recovery after dirty stop is not data loss — it's slow. If rows are missing after recovery, the writes were not committed before the kill.
