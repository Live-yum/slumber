#!/usr/bin/env python3
"""Audit the real executable before producing portable archives and SHA256."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def pe_imports(path):
    data = path.read_bytes()
    assert data[:2] == b"MZ", "Not a PE executable"
    pe = struct.unpack_from("<I", data, 0x3c)[0]
    assert data[pe:pe+4] == b"PE\0\0"
    machine, count = struct.unpack_from("<HH", data, pe+4)
    assert machine == 0x8664, "Expected AMD64 executable"
    optional_size = struct.unpack_from("<H", data, pe+20)[0]
    optional = pe+24
    assert struct.unpack_from("<H", data, optional)[0] == 0x20b
    base = struct.unpack_from("<Q", data, optional+24)[0]
    sections = []
    for index in range(count):
        start = optional + optional_size + index*40
        virtual_size, address, raw_size, raw = struct.unpack_from("<IIII", data, start+8)
        sections.append((address, max(virtual_size, raw_size), raw))
    def offset(rva):
        for address, size, raw in sections:
            if address <= rva < address+size:
                result = raw + rva-address
                assert result < len(data)
                return result
        raise ValueError("PE RVA outside mapped sections")
    def name(rva):
        start = offset(rva)
        end = data.index(b"\0", start, min(len(data), start+512))
        return data[start:end].decode("ascii")
    imports = set()
    for directory, width, name_index in [(1, 20, 3), (13, 32, 1)]:
        rva, size = struct.unpack_from("<II", data, optional+112+directory*8)
        if not rva:
            continue
        start = offset(rva)
        for index in range(size//width+1):
            fields = struct.unpack_from("<"+"I"*(width//4), data, start+index*width)
            if not any(fields):
                break
            name_rva = fields[name_index]
            if directory == 13 and not fields[0] & 1:
                name_rva -= base
            imports.add(name(name_rva))
    assert imports, "PE import audit found no modules"
    forbidden = ("vcruntime", "msvcp", "ucrtbase", "libcrypto", "libssl", "sqlite3", "libgcc", "libstdc++")
    assert not [dll for dll in imports if dll.lower().startswith(forbidden)], imports
    return sorted(imports)

def audit(binary, target):
    version = subprocess.check_output([str(binary), "--version"], text=True).strip()
    help_text = subprocess.check_output([str(binary), "--help"], text=True)
    request_help = subprocess.check_output([str(binary), "request", "--help"], text=True)
    assert "--portable" in help_text and "--transformed" in request_help
    result = {"target": target, "runner_os": platform.system(), "runner_machine": platform.machine(), "version": version,
              "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "cli_startup": "passed"}
    if target.endswith("windows-msvc"):
        result["pe_imports"] = pe_imports(binary)
        result["external_vc_runtime_imports"] = "none"
    else:
        program = subprocess.check_output(["readelf", "-lW", str(binary)], text=True)
        dynamic = subprocess.check_output(["readelf", "-dW", str(binary)], text=True)
        header = subprocess.check_output(["readelf", "-hW", str(binary)], text=True)
        assert "INTERP" not in program and "NEEDED" not in dynamic, "ELF is not self-contained/static"
        assert ("AArch64" in header) if target.startswith("aarch64") else ("X86-64" in header or "X86_64" in header)
        result.update(elf_interpreter="none", elf_needed="none", elf_header=header.strip())
    return result

def notices(destination):
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT))
    with (destination / "THIRD-PARTY-NOTICES.txt").open("w", encoding="utf-8") as output:
        output.write("Dependency license inventory (includes build/test dependencies conservatively).\n")
        for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
            output.write(f"\n=== {package['name']} {package['version']} | {package.get('license')} ===\n")
            root = Path(package["manifest_path"]).parent.resolve()
            candidates = list(root.glob("LICENSE*")) + list(root.glob("COPYING*"))
            if package.get("license_file"):
                candidates.append(root / package["license_file"])
            for path in sorted(set(candidates)):
                if path.is_file() and path.resolve().is_relative_to(root) and path.stat().st_size < 1_000_000:
                    output.write(path.read_text(encoding="utf-8", errors="replace"))
                    output.write("\n")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    report = audit(binary, args.target)
    output = ROOT / "dist/native-crypto"
    output.mkdir(parents=True, exist_ok=True)
    name = "slumber-native-crypto-" + args.target
    with tempfile.TemporaryDirectory() as temporary:
        package = Path(temporary) / name
        package.mkdir()
        shutil.copy2(binary, package / binary.name)
        shutil.copy2(ROOT / "LICENSE", package / "LICENSE")
        shutil.copy2(ROOT / "docs/native-crypto-zh.md", package / "README.zh-CN.md")
        for filename in ("slumber.yml", "config.yml", "demo-request.plain.json"):
            shutil.copy2(ROOT / "examples/native-crypto" / filename, package / filename)
        (package / "BUILD-INFO.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        shutil.copy2(ROOT / "docs/builtin-tools-zh.md", package / "BUILTIN-TOOLS.zh-CN.md")
        # UTF-8 BOM + CRLF for reliable opening in Windows text editors.
        quickstart = (ROOT / "docs/quickstart-zh.txt").read_text(encoding="utf-8")
        quickstart_bytes = ("\ufeff" + quickstart.replace("\n", "\r\n")).encode("utf-8")
        (package / "QUICKSTART.zh-CN.txt").write_bytes(quickstart_bytes)
        notices(package)
        if args.target.endswith("windows-msvc"):
            archive = output / (name + ".zip")
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
                for path in sorted(package.rglob("*")):
                    if path.is_file():
                        bundle.write(path, path.relative_to(package.parent))
        else:
            archive = output / (name + ".tar.gz")
            with tarfile.open(archive, "w:gz") as bundle:
                bundle.add(package, arcname=name)
        # Validate the shipped instructions, not just the staging directory.
        member = name + "/QUICKSTART.zh-CN.txt"
        if args.target.endswith("windows-msvc"):
            with zipfile.ZipFile(archive) as bundle:
                assert bundle.read(member) == quickstart_bytes
        else:
            with tarfile.open(archive) as bundle:
                with bundle.extractfile(member) as instructions:
                    assert instructions.read() == quickstart_bytes
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        (output / (name + ".sha256")).write_text(f"{digest}  {archive.name}\n", encoding="ascii")
        (output / (name + ".audit.json")).write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        print(json.dumps(report, ensure_ascii=False, indent=2))
        print(f"{digest}  {archive.name}")

if __name__ == "__main__":
    main()
