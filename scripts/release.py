#!/usr/bin/env python3
import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib


VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)")
SECTIONS = ("Behavior changes", "Fixes", "Known limitations")


def candidate(version):
    if not VERSION.fullmatch(version) or version == "0.0.0":
        raise ValueError("candidate must be a nonzero major.minor.patch version")
    return version


def read_toml(path):
    return tomllib.loads(path.read_text())


def workspace(root):
    manifest = read_toml(root / "Cargo.toml")
    packages = {}
    for member in manifest["workspace"]["members"]:
        directories = list(root.glob(member))
        if not directories:
            raise ValueError(f"workspace member does not exist: {member}")
        for directory in directories:
            package = read_toml(directory / "Cargo.toml")["package"]
            if package.get("version") != {"workspace": True}:
                raise ValueError(f"{member} must inherit the workspace version")
            packages[package["name"]] = directory
    for path in [root / "package.json", *root.glob("packages/*/package.json")]:
        package = json.loads(path.read_text())
        if package.get("private") and "version" in package:
            raise ValueError(f"{path.relative_to(root)} must not independently version a private npm package")
    return manifest["workspace"]["package"]["version"], packages


def check_lock(root, packages, version):
    locked = read_toml(root / "Cargo.lock")["package"]
    for name in packages:
        entries = [p for p in locked if p["name"] == name and "source" not in p]
        if len(entries) != 1 or entries[0]["version"] != version:
            raise ValueError(f"Cargo.lock version disagrees for {name}")


def notes_valid(notes):
    for section in SECTIONS:
        match = re.search(rf"^### {section}\n(.*?)(?=^### |\Z)", notes, re.M | re.S)
        if not match or not match[1].strip():
            raise ValueError(f"notes need a nonempty '{section}' section")


def validate(root, tag, browser_build=None):
    if not tag.startswith("v"):
        raise ValueError("tag must be v<product-version>")
    version = candidate(tag[1:])
    current, packages = workspace(root)
    if current != version:
        raise ValueError("tag and workspace version disagree")
    check_lock(root, packages, version)
    paths = [root / "packages/client/public/version.json"]
    if browser_build is not None:
        paths.append(browser_build / "version.json")
    for path in paths:
        if json.loads(path.read_text()) != {"version": version}:
            raise ValueError(f"browser version disagrees: {path}")
    changelog = (root / "CHANGELOG.md").read_text()
    entries = re.findall(r"^## \[([^\]]+)\]", changelog, re.M)
    if not entries or entries[0] != version or entries.count(version) != 1:
        raise ValueError("changelog must lead with exactly one entry for the candidate")
    notes = re.split(r"^## \[[^\]]+\].*\n", changelog, flags=re.M)[1]
    notes_valid(notes)
    existing = subprocess.run(["git", "-C", root, "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}"], capture_output=True, text=True)
    if existing.returncode == 0:
        head = subprocess.check_output(["git", "-C", root, "rev-parse", "HEAD"], text=True).strip()
        dirty = subprocess.check_output(["git", "-C", root, "status", "--porcelain", "--untracked-files=no"], text=True)
        if existing.stdout.strip() != head or dirty:
            raise ValueError("existing version tag refers to different content; prepare a patch candidate")
    return version


def prepare(root, version, notes_path):
    candidate(version)
    current, packages = workspace(root)
    check_lock(root, packages, current)
    existing = subprocess.run(["git", "-C", root, "show-ref", "--verify", f"refs/tags/v{version}"], capture_output=True)
    if existing.returncode == 0:
        raise ValueError("version tag already exists; choose a new patch candidate")
    notes = notes_path.read_text().strip() + "\n"
    notes_valid(notes)
    changelog_path = root / "CHANGELOG.md"
    changelog = changelog_path.read_text() if changelog_path.exists() else "# Changelog\n"
    if re.search(rf"^## \[{re.escape(version)}\]", changelog, re.M):
        raise ValueError("version already has reviewed notes; choose a new patch candidate")
    manifest_path = root / "Cargo.toml"
    manifest = manifest_path.read_text()
    manifest = re.sub(r'(\[workspace.package\]\s*\n(?:[^\[]*?))(^version\s*=\s*)"[^"]+"', lambda m: m[1] + m[2] + json.dumps(version), manifest, count=1, flags=re.M)
    lock_path = root / "Cargo.lock"
    blocks = lock_path.read_text().split("[[package]]")
    for i, block in enumerate(blocks[1:], 1):
        package = tomllib.loads(block)
        if package["name"] in packages and "source" not in package:
            blocks[i] = re.sub(r'^version = "[^"]+"', f'version = "{version}"', block, count=1, flags=re.M)
    manifest_path.write_text(manifest)
    lock_path.write_text("[[package]]".join(blocks))
    browser = root / "packages/client/public/version.json"
    browser.parent.mkdir(parents=True, exist_ok=True)
    browser.write_text(json.dumps({"version": version}) + "\n")
    header, _, history = changelog.partition("\n")
    changelog_path.write_text(f"{header}\n\n## [{version}]\n\n{notes}\n{history.lstrip()}")
    validate(root, f"v{version}")
    return version


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    commands = parser.add_subparsers(dest="command", required=True)
    prep = commands.add_parser("prepare")
    prep.add_argument("version")
    prep.add_argument("--notes", type=Path, required=True)
    check = commands.add_parser("validate")
    check.add_argument("tag")
    check.add_argument("--browser-build", type=Path)
    args = parser.parse_args()
    try:
        version = prepare(args.root, args.version, args.notes) if args.command == "prepare" else validate(args.root, args.tag, args.browser_build)
    except (ValueError, OSError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release: {error}\n")
    print(json.dumps({"version": version, "tag": f"v{version}"}))


if __name__ == "__main__":
    main()
