#!/usr/bin/env bash
# Starts an Enterprise IRIS with a license key, and a Web Gateway in front of it, for the e2e job.
#
# Enterprise images have no private web server, so Atelier REST is reached through a
# webgateway container that talks to IRIS's superserver. The key is mounted read-only and never
# printed; only its expiry date and capacity are logged.
#
#   IRIS_IMAGE=... GATEWAY_IMAGE=... KEY_FILE=/path/iris.key start-enterprise-iris.sh
#   IRIS_NAME=iris-e2e start-enterprise-iris.sh stop
#
# Optional: IRIS_NAME (iris-e2e), WEB_PORT (52773 on 127.0.0.1), IRIS_PASSWORD_VALUE (SYS).
set -euo pipefail

IRIS_NAME=${IRIS_NAME:-iris-e2e}
GW_NAME=${GW_NAME:-${IRIS_NAME}-gw}
WEB_PORT=${WEB_PORT:-52773}
PW=${IRIS_PASSWORD_VALUE:-SYS}

if [ "${1:-}" = "stop" ]; then
  docker rm -f "$GW_NAME" "$IRIS_NAME" >/dev/null 2>&1 || true
  exit 0
fi

: "${IRIS_IMAGE:?set IRIS_IMAGE}" "${GATEWAY_IMAGE:?set GATEWAY_IMAGE}" "${KEY_FILE:?set KEY_FILE}"
[ -r "$KEY_FILE" ] || { echo "KEY_FILE is not readable" >&2; exit 1; }

docker run -d --name "$IRIS_NAME" --hostname "$IRIS_NAME" \
  --shm-size 1g --ulimit nofile=65536:65536 \
  -v "$KEY_FILE:/usr/irissys/mgr/iris.key:ro" \
  "$IRIS_IMAGE" >/dev/null

for _ in $(seq 1 60); do
  docker exec "$IRIS_NAME" iris qlist 2>/dev/null | grep -q '\^running' && break
  sleep 3
done

# _SYSTEM gets a known password, CallIn is on (embedded Python and the native tests need it),
# the gateway logs in as CSPSystem with password authentication on %Service_WebGateway, and USER
# is an interoperability namespace, as it is on the Community image (the plain iris image leaves it
# off, so `Include Ensemble` and the production tests fail there).
out=""
for _ in $(seq 1 20); do
  out=$(docker exec -i "$IRIS_NAME" iris session IRIS -U %SYS <<OS
Set p("Password")="$PW",p("ChangePassword")=0,p("PasswordNeverExpires")=1 Write "PW:",##class(Security.Users).Modify("_SYSTEM",.p),!
Set c("Password")="$PW",c("PasswordNeverExpires")=1 Write "CSP:",##class(Security.Users).Modify("CSPSystem",.c),!
Set s("Enabled")=1 Write "CALLIN:",##class(Security.Services).Modify("%Service_CallIn",.s),!
Set g("Enabled")=1,g("AutheEnabled")=32 Write "GW:",##class(Security.Services).Modify("%Service_WebGateway",.g),!
Write "ENS:",##class(%EnsembleMgr).EnableNamespace("USER",0),!
Write "KEY: expires ",\$ZDATE(\$SYSTEM.License.KeyExpirationDate(),3),", ",\$SYSTEM.License.KeyLicenseCapacity(),!
Halt
OS
  ) || true
  if echo "$out" | grep -q 'PW:1' && echo "$out" | grep -q 'CSP:1' \
    && echo "$out" | grep -q 'CALLIN:1' && echo "$out" | grep -q 'GW:1' \
    && echo "$out" | grep -q 'ENS:1'; then
    break
  fi
  sleep 3
done
echo "$out" | grep -E '^(PW|CSP|CALLIN|GW|ENS|KEY):' || { echo "$out"; docker logs "$IRIS_NAME" 2>&1 | tail -30; exit 1; }
echo "$out" | grep -q 'PW:1' || { docker logs "$IRIS_NAME" 2>&1 | tail -30; exit 1; }

# The default bridge has no container DNS, so the gateway is pointed at the IRIS container IP.
# (A user network would need a free address pool, which a busy Docker host may not have.)
iris_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$IRIS_NAME")
conf=$(mktemp -d)
cat >"$conf/CSP.ini" <<INI
[SYSTEM]
SM_TIMEOUT=300
Server_Response_Timeout=300
Queued_Request_Timeout=300
No_Activity_Timeout=86400

[SYSTEM_INDEX]
LOCAL=Enabled

[LOCAL]
Ip_Address=$iris_ip
TCP_Port=1972
Minimum_Server_Connections=3
Maximum_Server_Connections=100
Connection_Security_Level=0
Username=CSPSystem
Password=$PW

[APP_PATH_INDEX]
/=Enabled

[APP_PATH:/]
Default_Server=LOCAL
INI
cat >"$conf/CSP.conf" <<'CONF'
CSPModulePath "${ISC_PACKAGE_INSTALLDIR}/bin/"
CSPConfigPath "${ISC_PACKAGE_INSTALLDIR}/bin/"

<Location "/">
    CSP On
    SetHandler csp-handler-sa
</Location>

<Directory "${ISC_PACKAGE_INSTALLDIR}/bin/">
    AllowOverride None
    Options None
    Require all granted
    <FilesMatch "\.(log|ini|pid|exe)$">
        Require all denied
    </FilesMatch>
</Directory>
CONF
chmod 644 "$conf/CSP.ini" "$conf/CSP.conf"

docker run -d --name "$GW_NAME" -p "127.0.0.1:$WEB_PORT:80" \
  -v "$conf/CSP.ini:/opt/webgateway/bin/CSP.ini" \
  -v "$conf/CSP.conf:/etc/apache2/conf-enabled/CSP.conf" \
  "$GATEWAY_IMAGE" >/dev/null

for i in $(seq 1 40); do
  status=$(curl -s -o /dev/null -w '%{http_code}' --connect-timeout 5 \
    -u "_SYSTEM:$PW" "http://127.0.0.1:$WEB_PORT/api/atelier/" || echo 000)
  if [ "$status" = "200" ]; then
    echo "Atelier REST answers through the gateway on 127.0.0.1:$WEB_PORT"
    exit 0
  fi
  echo "  gateway attempt $i: HTTP $status"
  sleep 3
done
docker logs "$GW_NAME" 2>&1 | tail -30
exit 1
