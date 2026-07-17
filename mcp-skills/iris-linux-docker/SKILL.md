---
name: iris-linux-docker
description: >
  IRIS Docker container volume permissions on Linux. All IRIS editions (community,
  enterprise, irishealth, ai_hub) run as UID 51773 (irisowner). Bind-mounting a
  host directory owned by a regular Linux user causes startup crash. Load when
  IRIS container fails on Linux with iris-main.log error, or when setting up
  any IRIS container with bind-mounted volumes on Linux.
tags: [iris, docker, linux, permissions, devtester]
---

# IRIS Docker — Linux Volume Permissions

## The Problem

ALL IRIS container editions run as UID 51773 (`irisowner`). On Linux, if you
bind-mount a host directory owned by UID 1000 (typical Linux user), the container
can read the volume but cannot write to it. IRIS needs write access at startup.

**Symptom** — container exits immediately with:
```
terminate called after throwing an instance of 'std::runtime_error'
what(): Unable to find/open file iris-main.log in current directory /home/irisowner/dev
```

**Affected**: All IRIS editions on Linux — community, enterprise, irishealth, ai_hub, light.
**Not affected**: macOS (VirtioFS translates permissions transparently).

Source: READY 2026 hackathon (Anthony Master, careconnect team).

---

## Fix Options

### Option 1 — POSIX ACLs (recommended)

Minimal footprint, no broad permission changes, new files inherit automatically:

```bash
setfacl -R -m u:51773:rwX <repo-dir>
setfacl -R -d -m u:51773:rwX <repo-dir>
```

The `-d` flag sets default ACL so new files/dirs created inside inherit the rule.
Verify: `getfacl <repo-dir>`

**If re-cloning**: the new clone directory needs these commands re-run.
Add to your project `Makefile` or `README` setup steps.

### Option 2 — tmpfs (no persistence needed)

```yaml
# docker-compose.yml
services:
  iris:
    volumes:
      - type: tmpfs
        target: /home/irisowner/dev
```

### Option 3 — chown on host (broad, simple)

```bash
sudo chown -R 51773:51773 <repo-dir>
```

Works but gives irisowner ownership of your source files on the host.

### Option 4 — Named Docker volume (avoid bind-mount)

```yaml
volumes:
  iris-data:
services:
  iris:
    volumes:
      - iris-data:/home/irisowner/dev
```

Data persists in Docker's managed storage, no host permission issues.

---

## iris-devtester Pattern

When using iris-devtester with a bind-mounted workspace on Linux:

```python
from iris_devtester import IRISContainer

container = (
    IRISContainer("intersystemsdc/iris-community:latest")
    .with_name("myapp-iris")
    .with_bind_mount("/home/user/myproject", "/home/irisowner/dev")
    .start()
)
# If this fails on Linux with iris-main.log error:
# Run: setfacl -R -m u:51773:rwX /home/user/myproject
#      setfacl -R -d -m u:51773:rwX /home/user/myproject
```

---

## Anti-Pattern

```yaml
# DO NOT do this on Linux without fixing permissions first:
volumes:
  - ./:/home/irisowner/dev   # Will fail if host dir owned by uid 1000
```
