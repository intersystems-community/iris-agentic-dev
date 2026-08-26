#!/usr/bin/env python3
"""Build a VS Code credential fixture for the check-sm-credential CI test.

Reproduces the layout and crypto that VS Code itself uses on Windows, so the
test exercises the real path rather than a stand-in:

    <root>/Local State                      DPAPI-sealed AES-256 key
    <root>/User/globalStorage/state.vscdb   the secret:// entry

VS Code stores JSON.stringify(safeStorage.encryptString(pw)), i.e. TEXT of the
form {"type":"Buffer","data":[...]}. On Windows the bytes inside are a Chromium
OSCrypt envelope: b"v10" + 12-byte nonce + AES-256-GCM ciphertext + 16-byte tag.
The AES key is base64 in Local State under os_crypt.encrypted_key, prefixed with
the ASCII bytes "DPAPI" and sealed with CryptProtectData.

Modes:
  safestorage  the current shape (Buffer JSON around a v10 envelope), as TEXT
  rawdpapi     legacy shape: raw DPAPI ciphertext in a BLOB column
  junk         an unrecognizable value, to check the error path

Sealing requires Windows. --self-test skips DPAPI and checks the rest anywhere.
"""

import argparse
import base64
import json
import os
import sqlite3
import sys

SECRET_KEY = (
    'secret://{{"extensionId":"intersystems-community.servermanager",'
    '"key":"credentialProvider:{server}/{user}"}}'
)
LOCAL_STATE_PREFIX = b"DPAPI"
NONCE_LEN = 12


def dpapi_protect(data: bytes) -> bytes:
    """Seal bytes with CryptProtectData (CurrentUser scope)."""
    import ctypes
    import ctypes.wintypes

    class DATA_BLOB(ctypes.Structure):
        _fields_ = [
            ("cbData", ctypes.wintypes.DWORD),
            ("pbData", ctypes.POINTER(ctypes.c_char)),
        ]

    buf = ctypes.create_string_buffer(data, len(data))
    blob_in = DATA_BLOB(len(data), ctypes.cast(buf, ctypes.POINTER(ctypes.c_char)))
    blob_out = DATA_BLOB()
    ok = ctypes.windll.crypt32.CryptProtectData(
        ctypes.byref(blob_in), None, None, None, None, 0, ctypes.byref(blob_out)
    )
    if not ok:
        raise OSError(f"CryptProtectData failed: {ctypes.GetLastError()}")
    try:
        return ctypes.string_at(blob_out.pbData, blob_out.cbData)
    finally:
        ctypes.windll.kernel32.LocalFree(blob_out.pbData)


def seal_v10(password: str, aes_key: bytes, nonce: bytes) -> bytes:
    """Build the Chromium OSCrypt v10 envelope for a password."""
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM

    sealed = AESGCM(aes_key).encrypt(nonce, password.encode("utf-8"), None)
    return b"v10" + nonce + sealed


def buffer_json(payload: bytes) -> str:
    """Match JSON.stringify(Buffer) exactly — compact, no spaces."""
    return json.dumps({"type": "Buffer", "data": list(payload)}, separators=(",", ":"))


def write_item_table(db_path: str, key: str, value) -> None:
    if os.path.exists(db_path):
        os.remove(db_path)
    db = sqlite3.connect(db_path)
    db.execute("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB NOT NULL)")
    db.execute("INSERT INTO ItemTable VALUES (?, ?)", (key, value))
    db.commit()
    db.close()


def build(root: str, server: str, user: str, password: str, mode: str) -> None:
    global_storage = os.path.join(root, "User", "globalStorage")
    os.makedirs(global_storage, exist_ok=True)
    db_path = os.path.join(global_storage, "state.vscdb")
    key = SECRET_KEY.format(server=server, user=user)

    if mode == "safestorage":
        aes_key = os.urandom(32)
        envelope = seal_v10(password, aes_key, os.urandom(NONCE_LEN))
        stored = buffer_json(envelope)
        local_state = {
            "os_crypt": {
                "encrypted_key": base64.b64encode(
                    LOCAL_STATE_PREFIX + dpapi_protect(aes_key)
                ).decode("ascii")
            }
        }
        with open(os.path.join(root, "Local State"), "w", encoding="utf-8") as fh:
            json.dump(local_state, fh)
        write_item_table(db_path, key, stored)
        print(f"safestorage: envelope {len(envelope)}B -> {len(stored)} chars of TEXT")

    elif mode == "rawdpapi":
        write_item_table(db_path, key, sqlite3.Binary(dpapi_protect(password.encode())))
        print("rawdpapi: raw DPAPI ciphertext in a BLOB column")

    elif mode == "junk":
        write_item_table(db_path, key, "not-real-dpapi-data")
        print("junk: unrecognizable TEXT value")

    else:
        raise SystemExit(f"unknown mode: {mode}")

    print(f"db: {db_path}")


def self_test() -> None:
    """Check the non-DPAPI half of the fixture on any platform."""
    aes_key = bytes(range(32))
    envelope = seal_v10("correct-horse1", aes_key, bytes(range(NONCE_LEN)))
    assert envelope.startswith(b"v10"), envelope[:4]
    # 3-byte tag + 12-byte nonce + len(pw) ciphertext + 16-byte GCM tag
    assert len(envelope) == 3 + NONCE_LEN + len("correct-horse1") + 16, len(envelope)

    stored = buffer_json(envelope)
    assert '"type":"Buffer"' in stored, stored[:40]
    assert json.loads(stored)["data"] == list(envelope)
    # The field report was a 187-character value; a low-teens password lands there.
    assert 150 <= len(stored) <= 230, len(stored)

    from cryptography.hazmat.primitives.ciphers.aead import AESGCM

    opened = AESGCM(aes_key).decrypt(envelope[3:15], envelope[15:], None)
    assert opened == b"correct-horse1", opened
    print(f"self-test OK: {len(envelope)}B envelope -> {len(stored)} chars of TEXT")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--root", help="directory to treat as the Code install root")
    ap.add_argument("--server", default="ci-server")
    ap.add_argument("--user", default="ci-user")
    ap.add_argument("--password", default="ci-test-password")
    ap.add_argument("--mode", choices=["safestorage", "rawdpapi", "junk"])
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        self_test()
        return 0
    if not args.root or not args.mode:
        ap.error("--root and --mode are required unless --self-test is given")
    if args.mode != "junk" and sys.platform != "win32":
        ap.error(f"mode {args.mode} needs Windows (CryptProtectData)")

    build(args.root, args.server, args.user, args.password, args.mode)
    return 0


if __name__ == "__main__":
    sys.exit(main())
