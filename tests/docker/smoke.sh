#!/usr/bin/env bash
# Docker smoke test for iris-agentic-dev image (SC-001, SC-002, SC-005).
# Usage: IMAGE=iris-agentic-dev-smoke bash tests/docker/smoke.sh
set -euo pipefail

IMAGE="${IMAGE:-iris-agentic-dev-smoke}"

echo "=== iris-agentic-dev Docker smoke test ==="
echo "Image: $IMAGE"

# SC-002: binary starts and exits 0 for --version
echo ""
echo "--- SC-002: --version ---"
docker run --rm "$IMAGE" --version
echo "OK: --version exit 0"

# SC-005: image size <= 20 MB
echo ""
echo "--- SC-005: image size ---"
SIZE=$(docker image inspect "$IMAGE" --format='{{.Size}}')
MAX=$((20 * 1024 * 1024))
if [ "$SIZE" -gt "$MAX" ]; then
  echo "FAIL: image size ${SIZE} bytes exceeds 20 MB limit ($(( SIZE / 1024 / 1024 )) MB)"
  exit 1
fi
echo "OK: image size ${SIZE} bytes ($(( SIZE / 1024 / 1024 )) MB)"

# SC-001: MCP initialize handshake over stdio
echo ""
echo "--- SC-001: MCP initialize ---"
RESPONSE=$(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke-test","version":"0.0.1"}}}\n' \
  | docker run --rm -i "$IMAGE" mcp 2>/dev/null | head -1)

echo "Response: $RESPONSE"

python3 - "$RESPONSE" << 'EOF'
import sys, json
raw = sys.argv[1]
try:
    data = json.loads(raw)
except json.JSONDecodeError as e:
    print(f"FAIL: response is not valid JSON: {e}")
    sys.exit(1)
r = data.get("result", {})
if "protocolVersion" not in r:
    print(f"FAIL: missing protocolVersion in result: {data}")
    sys.exit(1)
if "serverInfo" not in r:
    print(f"FAIL: missing serverInfo in result: {data}")
    sys.exit(1)
name = r["serverInfo"].get("name", "")
if name != "iris-agentic-dev":
    print(f"FAIL: unexpected serverInfo.name: {name!r}")
    sys.exit(1)
print(f"OK: MCP initialize valid (protocolVersion={r['protocolVersion']}, serverInfo.name={name!r})")
EOF

echo ""
echo "=== All smoke tests passed ==="
