---
name: iris-vscode-objectscript
description: >
  Use when configuring VSCode for ObjectScript development against an IRIS
  container. Covers the intersystems.servers settings.json config, the
  critical port 52773 vs 1972 distinction, the working webgateway pattern
  for enterprise images, three hard-won bugs from the 2026-05-03 session,
  and how to verify the connection.
  Load when: setting up iris-dev VSCode extension, getting 404 on /api/atelier/,
  connecting to enterprise IRIS, or setting up the webgateway container.
tags: [iris, vscode, objectscript, atelier, webserver, devtester, extension, webgateway, enterprise]
---

# VSCode ObjectScript Extension — IRIS Setup

## Which Images Have Atelier REST on Port 52773?

| Image | Has private web server | Atelier REST | Solution |
|---|---|---|---|
| `intersystemsdc/iris-community:*` | ✅ built-in | ✅ port 52773 | Direct |
| `intersystemsdc/irishealth-community:*` | ✅ built-in | ✅ port 52773 | Direct |
| `containers.intersystems.com/intersystems/iris:*` (enterprise) | ❌ WebServer=0 | ✅ via webgateway | See below |
| `irishealth:2026.2.0AI.*` | ❌ WebServer=0 | ✅ via webgateway | See below |

Enterprise images have `WebServer=0` and no httpd binary. **The webgateway container DOES work** — but requires correct configuration (see three bugs below).

---

## Enterprise + Webgateway: The Working Pattern

Verified working 2026-05-03 against `intersystems/iris:2026.1`.

### docker-compose

```yaml
services:
  iris:
    image: containers.intersystems.com/intersystems/iris:2026.1
    container_name: iris-enterprise
    ports:
      - "4972:1972"
    volumes:
      - ./iris.key:/usr/irissys/mgr/iris.key:ro
    networks:
      - iris-net

  webgateway:
    image: containers.intersystems.com/intersystems/webgateway:2026.1
    container_name: iris-enterprise-webgateway
    ports:
      - "64780:80"
    networks:
      - iris-net
    volumes:
      - ./webgateway-init.sh:/webgateway-init.sh:ro
    entrypoint: ["/bin/sh", "/webgateway-init.sh"]

networks:
  iris-net:
    driver: bridge
```

### webgateway-init.sh (all three bugs fixed)

```bash
#!/bin/sh
# Start the webgateway in background
/startWebGateway &

# BUG 1 FIX: Wait for CSP.ini to be fully initialized before patching
for i in $(seq 1 60); do
    grep -q "Configuration_Initialized" /opt/webgateway/bin/CSP.ini 2>/dev/null && break
    sleep 1
done

# BUG 2 FIX: Add credentials to [LOCAL] server section.
# Default tries CSPSystem which doesn't exist in fresh enterprise containers.
# Must add Username/Password so the webgateway can authenticate to IRIS.
sed -i '/^\[LOCAL\]/a Username=_SYSTEM\nPassword=SYS' /opt/webgateway/bin/CSP.ini

# Point LOCAL at the IRIS container (use Docker service name, not localhost)
sed -i 's/^Ip_Address=127\.0\.0\.1/Ip_Address=iris/' /opt/webgateway/bin/CSP.ini

# BUG 3 FIX: Use CSP On directive (not SetHandler csp-handler-sa).
# SetHandler in <Location> doesn't work — only CSP On routes correctly.
# This is the ISC official pattern from webgateway-examples.
cat > /etc/apache2/conf-enabled/CSP.conf << "EOF"
CSPModulePath "${ISC_PACKAGE_INSTALLDIR}/bin/"
CSPConfigPath "${ISC_PACKAGE_INSTALLDIR}/bin/"

<Location />
    CSP On
</Location>

<Directory "${ISC_PACKAGE_INSTALLDIR}/bin/">
    AllowOverride None
    Options None
    Require all granted
    <FilesMatch "\.(log|ini|pid|exe)$">
         Require all denied
    </FilesMatch>
</Directory>
EOF

apachectl graceful 2>/dev/null || true
wait
```

### After starting: unexpire passwords

Fresh enterprise containers require a password change on first login. Run once:

```bash
docker exec -i iris-enterprise bash << 'EOF'
iris session IRIS -U %SYS << 'IRISEOF'
Do ##class(Security.Users).UnExpireUserPasswords("*")
halt
IRISEOF
EOF
```

### Verify

```bash
curl -s -u "_SYSTEM:SYS" "http://localhost:64780/api/atelier/" \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['result']['content']['version'])"
# → IRIS for UNIX ... 2026.1 ...
```

### VSCode settings.json

```jsonc
{
  "intersystems.servers": {
    "iris-enterprise": {
      "webServer": {
        "host": "localhost",
        "port": 64780,      // webgateway host port
        "pathPrefix": ""
      },
      "username": "_SYSTEM",
      "description": "IRIS enterprise via webgateway"
    }
  }
}
```

---

## The Three Bugs (Hard-Won 2026-05-03)

### Bug 1: CSP.ini race condition

`/startWebGateway` writes CSP.ini asynchronously. If you `sed` it immediately, the file gets regenerated and your changes are lost. **Fix: poll for `Configuration_Initialized` in CSP.ini before patching.**

### Bug 2: Missing credentials in `[LOCAL]`

The webgateway's default `[LOCAL]` server section has no `Username`/`Password`. It tries to connect as `CSPSystem`, which doesn't exist in a fresh enterprise container. Result: `"Connection Validation Failed: 403 Access Denied"` in CSP.log. **Fix: add `Username=_SYSTEM` and `Password=SYS` to the `[LOCAL]` section.**

### Bug 3: Wrong Apache directive

`SetHandler csp-handler-sa` inside `<Location>` blocks does NOT route requests through the CSP module correctly — Apache's filesystem handler intercepts first and returns 404. The correct directive is `CSP On` inside `<Location />`. **This is the official ISC pattern from `intersystems-community/webgateway-examples`.** Using `SetHandler` is a dead end.

### Diagnosis via CSP.log

```bash
docker exec iris-enterprise-webgateway tail -20 /opt/webgateway/logs/CSP.log
```

- `"Connection Validation Failed: 403 Forbidden... Access Denied"` → Bug 2 (missing credentials)
- `"Connection Validation Failed: 403... Password change required"` → need `UnExpireUserPasswords`
- No request entries at all → Bug 3 (wrong Apache directive, CSP module never fires)

---

## Community (Direct, No Webgateway)

```yaml
services:
  iris:
    image: intersystemsdc/iris-community:2026.1
    ports:
      - "1972:1972"
      - "52773:52773"
    environment:
      - ISC_CPF_MERGE_FILE=/tmp/merge.cpf
    volumes:
      - ./merge.cpf:/tmp/merge.cpf:ro
```

```
# merge.cpf — prevents password expiry prompt
[Actions]
ModifyUser:Name=_SYSTEM,ChangePassword=0,PasswordNeverExpires=1
ModifyUser:Name=SuperUser,ChangePassword=0,PasswordNeverExpires=1
```

VSCode settings.json uses `"port": 52773` (or the mapped host port).

---

## Verify Before Opening VSCode

```bash
curl -s -u "_SYSTEM:SYS" "http://localhost:52773/api/atelier/" \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['result']['content']['version'])"
# 404 → web server not running (enterprise without webgateway, or IRIS still starting)
# 401 → wrong credentials or expired password
# 500 → webgateway connecting but auth failing (check CSP.log)
# 200 + version string → working
```
