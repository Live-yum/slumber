#!/usr/bin/env python3
"""Check the Git index, never print secret values. Python stdlib only."""
import fnmatch
import re
import subprocess
import sys

# ponytail: denylist catches known labels/token formats, not arbitrary secrets;
# add a mature secret scanner if broader detection is required.
PATTERN = (
    "\u6df1\u6d77[ _-]?(\u4e00|1)\u53f7"
    "|[zZ][jJ][qQ]|[zZ]ijinqiao|\u7d2b\u91d1\u6865"
    r"|gh[pousr]_[A-Za-z0-9]{30,}"
    r"|github_pat_[A-Za-z0-9_]{40,}"
    r"|AKIA[A-Z0-9]{16}"
    r"|-----BEGIN (RSA |EC |OPENSSH |DSA |ENCRYPTED )?PRIVATE KEY-----"
    r"|https?://[^ /:@]+:[^ /@]+@"
)
PRIVATE_PATHS = (
    "slumber.yml", "slumber.yaml", "config.yml", "config.yaml",
    "private/*", "data/*", "log/*", "tmp/*", "state/*", "dist/*",
    "*.local.yml", "*.local.yaml", "*.private.yml", "*.private.yaml",
    "*.secret.yml", "*.secret.yaml", "*.pem", "*.key", "*.p12", "*.pfx",
    "*.sqlite", "*.sqlite3", "*.sqlite-*", "*.log",
)


def private_path(path):
    basename = path.rsplit("/", 1)[-1]
    return (basename == ".env" or basename.startswith(".env.") and basename != ".env.example"
            or any(fnmatch.fnmatchcase(path, pattern) for pattern in PRIVATE_PATHS))


def main():
    if sys.argv[1:] == ["--self-test"]:
        assert re.search(PATTERN, "\u6df1\u6d77\u4e00\u53f7")
        assert re.search(PATTERN, "z" + "jq")
        assert re.search(PATTERN, "ghp_" + "a" * 36)
        assert re.search(PATTERN, "https://" + "user:password" + "@example.invalid")
        assert not re.search(PATTERN, "DEMO-SITE public-password 0123456789abcdef")
        assert private_path("slumber.yml") and private_path("private/customer.json")
        assert private_path("examples/.env.production")
        assert not private_path("slumber.example.yml")
        assert not private_path("examples/native-crypto/slumber.yml")
        assert not private_path(".env.example")
        print("PASS: sensitive-data guard self-check")
        return 0
    if sys.argv[1:]:
        raise SystemExit("Usage: check_sensitive.py [--self-test]")
    paths = subprocess.check_output(["git", "ls-files", "-z"]).decode("utf-8").split("\0")
    blocked = {path for path in paths if path and (private_path(path) or re.search(PATTERN, path))}
    result = subprocess.run(
        ["git", "grep", "--cached", "-I", "-l", "-z", "-E", PATTERN, "--", "."],
        capture_output=True, check=False,
    )
    if result.returncode not in (0, 1):
        raise SystemExit("Sensitive-data scan failed; commit blocked")
    blocked.update(filter(None, result.stdout.decode("utf-8").split("\0")))
    for path in sorted(blocked):
        print(f"BLOCKED: {path}", file=sys.stderr)
    if not blocked:
        print("PASS: Git index has no blocked private files or known sensitive patterns")
    return int(bool(blocked))


if __name__ == "__main__":
    sys.exit(main())
