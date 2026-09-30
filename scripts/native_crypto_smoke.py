#!/usr/bin/env python3
"""Development/CI-only interoperability tests; not a binary runtime dependency.
All test requests go to a local mock server. Install cryptography and PyYAML.
"""
from __future__ import annotations
import argparse
import base64
import contextlib
import http.server
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import threading
import urllib.parse
import yaml
from cryptography.hazmat.primitives import padding
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

ROOT = Path(__file__).resolve().parents[1]
KEYS = {128: b"0123456789abcdef", 192: b"0123456789abcdef01234567", 256: b"0123456789abcdef0123456789abcdef"}
IV = b"fedcba9876543210"
# Deliberately invalid personal identifiers; public test strings only.
PHONE, IDCARD = "00000000000", "000000000000000000"

class Codec:
    def __init__(self, name="aes-128-cbc", transport="base64"):
        self.name, self.transport = name, transport

    def configuration(self):
        config = {"algorithm": self.name}
        if self.name.startswith("aes-"):
            _, bits, mode = self.name.split("-")
            config.update(key={"value": KEYS[int(bits)].decode(), "encoding": "utf8"}, padding="pkcs7", plaintext_encoding="utf8", ciphertext_encoding=self.transport)
            if mode == "cbc":
                config["iv"] = {"value": IV.decode(), "encoding": "utf8"}
        if self.name != "none":
            config["base64_decode"] = {"ignore_ascii_whitespace": True, "allow_missing_padding": True}
        return config

    def cipher(self, am=False):
        _, bits, mode = self.name.split("-")
        return Cipher(algorithms.AES(KEYS[int(bits)]), modes.CBC(KEYS[128] if am else IV) if mode == "cbc" else modes.ECB())

    def encode(self, data, am=False):
        if self.name == "none":
            return data
        if self.name.startswith("aes-"):
            padder = padding.PKCS7(128).padder()
            data = padder.update(data) + padder.finalize()
            encryptor = self.cipher(am).encryptor()
            data = encryptor.update(data) + encryptor.finalize()
        url = self.name == "base64url" or (self.name.startswith("aes-") and self.transport == "base64url")
        return base64.urlsafe_b64encode(data) if url else base64.b64encode(data)

    def decode(self, data, am=False):
        if self.name == "none":
            return data
        data = b"".join(data.split())
        data += b"=" * (-len(data) % 4)
        url = self.name == "base64url" or (self.name.startswith("aes-") and self.transport == "base64url")
        data = base64.b64decode(data, altchars=b"-_" if url else None, validate=True)
        if self.name.startswith("aes-"):
            decryptor = self.cipher(am).decryptor()
            data = decryptor.update(data) + decryptor.finalize()
            unpadder = padding.PKCS7(128).unpadder()
            data = unpadder.update(data) + unpadder.finalize()
        return data

AM = Codec()

class MockServer(http.server.ThreadingHTTPServer):
    daemon_threads = True
    def __init__(self, port=0):
        super().__init__(("127.0.0.1", port), Handler)
        self.codec, self.calls, self.failures = Codec(), [], []

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass  # Never log complete URLs or credentials.

    def send_bytes(self, data, mime="application/json", status=200):
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def send_json(self, value, encoded=False):
        data = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()
        if encoded:
            data = self.server.codec.encode(b"\xef\xbb\xbf" + data)
            if self.server.codec.name != "none":
                data = b" \t\r\n" + data.rstrip(b"=") + b"\x0b\x0c"
        self.send_bytes(data, "text/plain" if encoded else "application/json")

    def route(self, method):
        parsed = urllib.parse.urlsplit(self.path)
        path, q = parsed.path, urllib.parse.parse_qs(parsed.query, keep_blank_values=True)
        self.server.calls.append((method, path))
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        try:
            if method == "POST" and path == "/api/auth/oauth/user/noValidCode/token":
                assert self.headers["client_id"] == "DEMO-CLIENT"
                assert self.headers["client_secret"] == "DEMO-SECRET"
                assert urllib.parse.parse_qs(body.decode()) == {"username": ["demo"], "password": ["public-password"]}
                self.send_json({"access_token": "AM-TOKEN", "expires_in": 3600})
            elif method == "GET" and path in ("/api/am/apiUser/getAllUserInfo", "/mock/am-persons-string"):
                assert self.headers["Authorization"] == "Bearer AM-TOKEN"
                if path == "/api/am/apiUser/getAllUserInfo":
                    assert q["destination"] == ["DEMO-SITE"] and q["pageSize"] == ["100"]
                records = [
                    {"userId": "DEMO-001", "fullName": "测试人员", "phone": AM.encode(PHONE.encode(), True).decode(), "idcardNo": AM.encode(IDCARD.encode(), True).decode()},
                    {"phone": None, "idcardNo": "  \n"}, {},
                ]
                value = json.dumps(records, ensure_ascii=False) if path.endswith("string") else records
                self.send_json({"code": 200, "data": {"list": value}, "large_id": 9007199254740993, "huge": 1844674407370955161600001})
            elif method == "POST" and path == "/personaccess/open/am/person":
                records = json.loads(body)
                if isinstance(records, dict):
                    records = [records]
                assert records
                for record in records:
                    assert AM.decode(record["phone"].encode(), True).decode() == PHONE
                    assert AM.decode(record["idcardNo"].encode(), True).decode() == IDCARD
                    assert record["fullName"] == "测试人员"
                self.send_json({"ok": True, "count": len(records)})
            elif path == "/parameters":
                expected = "7Uf+4FRcP6fdBw1EuG6Y2Q=="
                assert q["encrypted"] == [expected] and self.headers["x-encrypted"] == expected
                assert "%2B" in parsed.query.upper()
                self.send_json({"ok": True})
            elif method == "GET" and path == "/api/userlogin":
                assert q == {"user": ["demo"], "pass": ["public-password"]}
                assert not self.headers.get("Authorization")
                self.send_json({"code": 0, "token": "DEMO-TOKEN"}, True)
            elif method == "GET" and path in ("/api/realdata", "/api/realdir", "/api/hisdata", "/api/alarmdata", "/api/userlogout"):
                assert q["token"] == ["DEMO-TOKEN"] and not self.headers.get("Authorization")
                value = {"items": [{"name": "A1.PV", "value": 12.5}], "large_id": 9007199254740993, "huge": 1844674407370955161600001, "marker": "DECODED-VIEW"}
                if path != "/api/realdir":
                    value["code"] = 0
                self.send_json(value, True)
            elif method == "POST" and path == "/encrypted-echo":
                assert self.headers["Content-Type"] == "text/plain"
                decoded = self.server.codec.decode(body)
                assert json.loads(decoded)["large_id"] == 9007199254740993
                assert self.server.codec.encode(decoded) == body  # Independent exact comparison.
                self.send_bytes(self.server.codec.encode(decoded), "text/plain")
            elif method == "POST" and path == "/plain-echo":
                assert "application/json" in self.headers["Content-Type"]
                self.send_json(json.loads(body))
            elif path == "/invalid":
                self.send_bytes(b"BAD-PRIVATE-CIPHERTEXT!", "text/plain")
            else:
                self.server.failures.append("unexpected route")
                self.send_bytes(b'{"error":"unexpected route"}', status=400)
        except (AssertionError, ValueError, KeyError) as error:
            self.server.failures.append(f"{method} {path}: {type(error).__name__}")
            self.send_bytes(b'{"error":"local interoperability assertion failed"}', status=400)

    def do_GET(self):
        self.route("GET")
    def do_POST(self):
        self.route("POST")


def run_smoke(binary):
    binary = binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="slumber-crypto-test-") as temp:
        root = Path(temp)
        portable = root / "中文 空格 portable"
        portable.mkdir()
        executable = portable / binary.name
        shutil.copy2(binary, executable)
        executable.chmod(0o755)
        elsewhere = root / "other working directory"
        elsewhere.mkdir()
        shutil.copy2(ROOT / "examples/native-crypto/demo-request.plain.json", portable)
        (portable / "config.yml").write_text("follow_redirects: false\npersist: false\n", encoding="utf-8")
        config = yaml.safe_load((ROOT / "examples/native-crypto/slumber.yml").read_text(encoding="utf-8"))
        with MockServer() as server:
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                url = f"http://127.0.0.1:{server.server_port}"
                data = config["profiles"]["local"]["data"]
                for key in ("am_base_url", "gateway_url", "demo_b_base_url"):
                    data[key] = url
                config["crypto"]["unused"] = {"algorithm": "aes-128-ecb", "key": {"value": "{{ env('MISSING_UNUSED_AES_KEY') }}"}}
                config["requests"]["invalid"] = {"method": "GET", "url": url + "/invalid", "persist": False, "response_transform": [{"type": "decode", "crypto": "demo_b", "scope": "body", "parse": "json"}]}
                config["requests"]["raw_chain"] = {"method": "POST", "url": url + "/plain-echo", "persist": False, "body": {"type": "json", "data": {"phone": "{{ response('am_persons', trigger='no_history') | jsonpath('$.data.list[0].phone') | decrypt('am') }}"}}}
                collection = portable / "slumber.yml"
                def save():
                    collection.write_text(json.dumps(config, ensure_ascii=False), encoding="utf-8")
                env = {k: v for k, v in os.environ.items() if not (k.startswith(("AM_AES_", "DEMO_B_AES_")) or k in ("MISSING_UNUSED_AES_KEY", "SLUMBER_DATA_DIRECTORY", "SLUMBER_CONFIG_PATH"))}
                env = {k: v for k, v in env.items() if k.upper() not in ("PATH", "EDITOR", "VISUAL", "PAGER")}
                env.update(PATH="", EDITOR="missing-editor", VISUAL="missing-editor", PAGER="missing-pager")
                sql = subprocess.run([str(executable), "--portable", "db", "SELECT 9007199254740993 AS id;"], cwd=elsewhere, env=env, capture_output=True, timeout=45)
                assert sql.returncode == 0 and b"9007199254740993" in sql.stdout, sql.stderr
                def invoke(recipe, *extra, transformed=True, expected=0):
                    command = [str(executable), "--portable", "request", recipe]
                    if transformed:
                        command.append("--transformed")
                    result = subprocess.run(command + list(extra), cwd=elsewhere, env=env, capture_output=True, timeout=45)
                    assert result.returncode == expected, (recipe, result.returncode, result.stderr.decode(errors="replace"))
                    assert not server.failures, server.failures
                    return result
                save()
                raw = invoke("am_persons", transformed=False).stdout
                assert b"zf+as5WzK/EFJ0gG1RkRGA==" in raw and PHONE.encode() not in raw
                for recipe in ("am_persons", "am_persons_string_list"):
                    value = json.loads(invoke(recipe).stdout)
                    assert value["data"]["list"][0]["phone"] == PHONE
                    assert value["data"]["list"][0]["idcardNo"] == IDCARD
                    assert value["large_id"] == 9007199254740993 and value["huge"] == 1844674407370955161600001
                for recipe in ("am_push_person", "am_push_array", "encrypted_parameters"):
                    assert json.loads(invoke(recipe).stdout)["ok"]
                assert json.loads(invoke("raw_chain").stdout)["phone"] == PHONE
                assert json.loads(invoke("plain_json_echo").stdout)["message"] == "普通 JSON 原样发送"
                combinations = [(name, "base64") for name in ("none", "base64", "base64url")]
                combinations += [(f"aes-{bits}-{mode}", transport) for bits in (128, 192, 256) for mode in ("cbc", "ecb") for transport in ("base64", "base64url")]
                for name, transport in combinations:
                    server.codec = Codec(name, transport)
                    config["crypto"]["demo_b"] = server.codec.configuration()
                    save()
                    value = json.loads(invoke("demo_b_realdata").stdout)
                    assert value["marker"] == "DECODED-VIEW" and value["large_id"] == 9007199254740993
                    assert value["huge"] == 1844674407370955161600001
                    assert json.loads(invoke("encrypted_echo_demo").stdout) == json.loads((portable / "demo-request.plain.json").read_bytes())
                assert "code" not in json.loads(invoke("demo_b_realdir").stdout)
                for recipe in ("demo_b_hisdata", "demo_b_alarmdata", "demo_b_logout"):
                    assert json.loads(invoke(recipe).stdout)["code"] == 0
                output = portable / "result 中文.json"
                invoke("am_persons", "--output", str(output))
                assert json.loads(output.read_bytes())["data"]["list"][0]["phone"] == PHONE
                output.write_bytes(b"previous-complete-result")
                failed = invoke("invalid", "--output", str(output), expected=3)
                assert not failed.stdout and output.read_bytes() == b"previous-complete-result"
                assert b"BAD-PRIVATE-CIPHERTEXT" not in failed.stderr
                assert invoke("invalid", transformed=False).stdout == b"BAD-PRIVATE-CIPHERTEXT!"
                alternative = elsewhere / "alternative.yml"
                alternative.write_text('requests: {different: {method: GET, url: "' + url + '/invalid"}}', encoding="utf-8")
                result = subprocess.run([str(executable), "--portable", "-f", str(alternative), "request", "different"], cwd=elsewhere, env=env, capture_output=True, timeout=45)
                assert result.returncode == 0 and result.stdout == b"BAD-PRIVATE-CIPHERTEXT!"
                assert (portable / "data/state.sqlite").is_file()
                logs = list((portable / "log").glob("*.log"))
                assert logs
                for file in logs:
                    text = file.read_bytes()
                    assert KEYS[128] not in text and PHONE.encode() not in text and b"DEMO-TOKEN" not in text
                assert not list(elsewhere.glob("*.sqlite")) and not list(elsewhere.glob("*.log"))
                # A Connection context manages transactions, not its lifetime.
                # Close even on assertion failure so Windows can delete the DB.
                with contextlib.closing(sqlite3.connect(portable / "data/state.sqlite")) as db:
                    for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall():
                        for row in db.execute('SELECT * FROM "' + table.replace('"', '""') + '"'):
                            for value in row:
                                if isinstance(value, bytes):
                                    assert PHONE.encode() not in value and b"DEMO-TOKEN" not in value
            finally:
                server.shutdown()
                thread.join(timeout=5)
    print(f"PASS: {len(combinations)} codec/transport combinations, independent request mirrors, login chains, large numbers, raw/transformed output, Chinese portable paths and atomic failure output; resources closed and temporary directory removed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--serve", action="store_true")
    parser.add_argument("--port", type=int, default=18080)
    args = parser.parse_args()
    if args.serve:
        with MockServer(args.port) as server:
            print(f"Public-test-data-only mock: http://127.0.0.1:{server.server_port}")
            with contextlib.suppress(KeyboardInterrupt):
                server.serve_forever()
    elif args.binary:
        run_smoke(args.binary)
    else:
        parser.error("provide --binary or --serve")

if __name__ == "__main__":
    main()
