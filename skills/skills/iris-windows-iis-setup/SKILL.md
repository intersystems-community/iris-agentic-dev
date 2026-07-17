---
name: iris-windows-iis-setup
description: Configure IIS for IRIS installed natively on Windows so iris-agentic-dev can connect
---

# Skill: iris-windows-iis-setup

**Type**: How-to guide
**Applies to**: IRIS installed natively on Windows (no Docker), connecting via IIS

## Overview

When IRIS is installed directly on Windows (not in Docker), it uses IIS as its web server (IRIS 2024.1+) or the bundled Private Web Server on port 52773 (pre-2024.1). This guide covers the full IIS configuration procedure so `iris-agentic-dev` can connect.

The #1 failure mode: `/api/atelier` returns 404 even when the Management Portal loads correctly. This means the `/api` web application is missing from IIS.

---

## Step 1: Verify the `/api` web application exists in IIS Manager

Open **IIS Manager** (run `inetmgr`). Expand the server → **Sites** → **Default Web Site**. Look for an application named `api` in the tree.

**If it is missing**, add it:

1. Right-click **Default Web Site** → **Add Application**
2. Alias: `api`
3. Physical path: the Web Gateway bin directory, typically `C:\InterSystems\IRIS\CSP\bin`
4. Click OK

Then add a wildcard script handler:

1. Click the new `api` application → **Handler Mappings** → **Add Wildcard Script Map**
2. Request path: `*`
3. Executable: full path to `CSPms.dll`, e.g. `C:\InterSystems\IRIS\CSP\bin\CSPms.dll`
4. Name: `CSP`
5. Click OK → allow the handler when prompted

**Verification**: `curl http://localhost/api/atelier/` should return a JSON response (not 404).

---

## Step 2: Verify `CSP.ini` has an `[APP_PATH:/api]` entry

Open `CSP.ini` — typically at `C:\InterSystems\IRIS\CSP\bin\CSP.ini` (may vary by install; check IIS Manager physical path for the Web Gateway application).

Look for a section like:

```ini
[APP_PATH:/api]
DEFAULT_SERVER=LOCAL
```

If missing, add it (the IRIS installer normally does this; IIS reconfiguration may remove it).

**Verification**: The section exists in the file. Restart the IIS application pool after any `CSP.ini` change.

---

## Step 3: Confirm the correct port

| IRIS version | Web server               | Default port |
| ------------ | ------------------------ | ------------ |
| IRIS 2024.1+ | IIS                      | 80           |
| Pre-2024.1   | Private Web Server (PWS) | 52773        |

If you are on IRIS 2024.1+ and IIS is configured, use port 80. If you are on an older version and the PWS is still running, use 52773.

**Verification**: `curl http://localhost:<port>/api/atelier/` returns JSON with an `"api"` version field.

---

## Step 4: Use `127.0.0.1` if you see per-connection delays

Older Web Gateway builds have a known issue where using `localhost` as the hostname causes a 10061 error before each connection (visible as a ~1s delay per request). If you see this, switch to `127.0.0.1`:

```toml
# .iris-agentic-dev.toml
host = "127.0.0.1"
web_port = 80
```

**Verification**: No per-connection delay; requests complete immediately.

---

## Step 5: Minimal `.iris-agentic-dev.toml` for native IRIS

Create `.iris-agentic-dev.toml` in your project root:

```toml
# Native IRIS on Windows — no Docker required
host = "localhost"          # or "127.0.0.1" for older Web Gateway builds
web_port = 80               # IIS default (IRIS 2024.1+); use 52773 for pre-2024.1 PWS
namespace = "USER"          # your target namespace
```

**Verification**: Run `check_config` via `iris-agentic-dev`. The response should show:

- `"connected": true`
- `"connection_source": "http"`
- `"host": "localhost"` (or `"127.0.0.1"`)

If `connected` is false, recheck Steps 1–4. If `connection_source` is `"docker"`, the config file is not being picked up — verify the `.iris-agentic-dev.toml` path matches the workspace root.
