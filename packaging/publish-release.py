#!/usr/bin/env python3
"""Prepare/sign verified local artifacts and publish GitHub Releases without Actions."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import plistlib

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "froiibn/FTA"
TARGETS = {"darwin-aarch64", "darwin-x86_64", "windows-x86_64"}


def run(args, **kwargs):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, **kwargs)


def digest(path):
    with path.open("rb") as stream:
        result = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
        return result.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=sorted(TARGETS), required=True)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--installer", type=Path, required=True)
    parser.add_argument("--build-info", type=Path, required=True)
    parser.add_argument("--notes", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--key", type=Path, default=Path.home() / ".config/fta/update-signing/desktop.key")
    parser.add_argument("--gh", default=shutil.which("gh"))
    parser.add_argument("--publish", action="store_true")
    args = parser.parse_args()
    config = json.loads((ROOT / "apps/desktop/src-tauri/tauri.conf.json").read_text())
    version = config["version"]
    tag = f"v{version}"
    source = run(["git", "rev-parse", "HEAD"], capture_output=True).stdout.strip()
    tagged = subprocess.run(["git", "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}"], cwd=ROOT, capture_output=True, text=True)
    if tagged.returncode == 0:
        source = tagged.stdout.strip()
    info = json.loads(args.build_info.read_text(encoding="utf-8-sig"))
    # Release publishing is allowed only after native build evidence was supplied, never a cross-build guess.
    if info.get("version") != version or info.get("source_commit") != source:
        raise ValueError("Build evidence version/source must match the clean source being published")
    if info.get("target") != args.target or not info.get("native_checks_passed"):
        raise ValueError("Matching native OS verification evidence is required")
    if info.get("artifact_sha256") != digest(args.artifact) or info.get("installer_sha256") != digest(args.installer):
        raise ValueError("Native build evidence does not match these exact artifact bytes")
    if run(["git", "status", "--porcelain"], capture_output=True).stdout.strip():
        raise ValueError("Commit source and documentation before preparing release artifacts")
    if args.artifact.resolve().is_relative_to(ROOT / "apps"):
        raise ValueError("Use a staged release artifact, not application source")
    expected_suffix = "-setup.exe" if args.target.startswith("windows") else ".app.tar.gz"
    if not args.artifact.name.endswith(expected_suffix):
        raise ValueError("Updater artifact type does not match the target")
    if args.target.startswith("darwin") and not info.get("notarization", {}).get("accepted"):
        raise ValueError("macOS artifacts must be notarized before signing the final archive")
    if args.target.startswith("darwin"):
        with tarfile.open(args.artifact, "r:gz") as archive:
            members = {m.name.removeprefix("./"): m for m in archive.getmembers()}
            if any(Path(name).is_absolute() or ".." in Path(name).parts for name in members):
                raise ValueError("Unsafe updater archive paths")
            bundle = plistlib.load(archive.extractfile(members["FTA.app/Contents/Info.plist"]))
            if bundle["CFBundleShortVersionString"] != version or bundle["CFBundleIdentifier"] != config["identifier"]:
                raise ValueError("Updater archive bundle identity/version mismatch")
            for binary in ("fta-desktop", "fta-engine", "fta-mcp"):
                if "FTA.app/Contents/MacOS/" + binary not in members:
                    raise ValueError("Updater archive is missing an installed component")
    # The same private signing authority can sign a Windows-native artifact on the publishing Mac.
    # The key stays outside Git; the manifest contains only a public signature bound to this version.
    if args.key.resolve().is_relative_to(ROOT):
        raise ValueError("The updater private key must remain outside the repository")
    signing_env = dict(os.environ)
    signing_env.setdefault("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "")
    run(["npm", "--prefix", "apps/desktop", "run", "tauri", "--", "signer", "sign",
         "--private-key-path", str(args.key.resolve()), "--app-version", version,
         str(args.artifact.resolve())], stdout=subprocess.DEVNULL, env=signing_env)
    signature = args.artifact.with_name(args.artifact.name + ".sig").read_text().strip()
    # Reject incomplete/legacy signatures before publishing a manifest that requires signed versions.
    decoded = base64.b64decode(signature, validate=True).decode()
    if f"version:{version}" not in decoded and f"version: {version}" not in decoded:
        raise ValueError("The updater signature is not bound to the release version")
    verify_env = dict(os.environ, FTA_TEST_UPDATER_ARTIFACT=str(args.artifact.resolve()))
    run(["cargo", "test", "--locked", "-p", "fta-desktop", "signed_release_rejects_corruption_and_version_tampering",
         "--", "--ignored"], env=verify_env)
    args.output.mkdir(parents=True, exist_ok=True)
    assets = []
    for file in dict.fromkeys([args.artifact, args.artifact.with_name(args.artifact.name + ".sig"), args.installer, args.build_info]):
        staged = args.output / file.name
        if staged.resolve() != file.resolve():
            shutil.copy2(file, staged)
        assets.append(staged)
    manifest = args.output / f"latest-{args.target}.json"
    manifest.write_text(json.dumps({
        "version": version, "notes": args.notes.read_text(), "pub_date": info["built_at"],
        "url": f"https://github.com/{REPOSITORY}/releases/download/{tag}/{args.artifact.name}",
        "signature": signature,
    }, indent=2) + "\n")
    assets.append(manifest)
    hashes = args.output / f"SHA256SUMS-{args.target}-{version}.txt"
    hashes.write_text("".join(f"{digest(f)}  {f.name}\n" for f in assets))
    assets.append(hashes)
    print(f"Prepared {tag} {args.target}: {len(assets)} files")
    if not args.publish:
        return
    if not args.gh:
        raise ValueError("Authenticated GitHub CLI is required to publish")
    gh = [args.gh, "--repo", REPOSITORY]
    # Never overwrite a public installer/tag. Build all assets into a draft, then expose them together.
    check = subprocess.run(gh + ["release", "view", tag, "--json", "isDraft,assets"], cwd=ROOT, capture_output=True, text=True)
    public = check.returncode == 0 and not json.loads(check.stdout)["isDraft"]
    if public:
        existing_names = {asset["name"] for asset in json.loads(check.stdout)["assets"]}
        if any(file.name in existing_names for file in assets):
            raise ValueError("Never overwrite public assets; only a new native target may be added")
    if check.returncode != 0:
        run(gh + ["release", "create", tag, "--draft", "--target", source,
                  "--title", f"FTA {version}", "--notes-file", str(args.notes.resolve())])
    # Retain independently versioned target feeds when only one native OS is being released today.
    latest = subprocess.run(gh + ["release", "view", "--json", "tagName"], cwd=ROOT, capture_output=True, text=True)
    if not public and latest.returncode == 0:
        previous = json.loads(latest.stdout)["tagName"]
        for target in TARGETS - {args.target}:
            name = f"latest-{target}.json"
            result = subprocess.run(gh + ["release", "download", previous, "--pattern", name,
                                         "--dir", str(args.output.resolve()), "--skip-existing"],
                                    cwd=ROOT, capture_output=True, text=True)
            if result.returncode == 0:
                carried = args.output / name
                data = json.loads(carried.read_text())
                if not data.get("signature") or not data.get("url", "").startswith(f"https://github.com/{REPOSITORY}/releases/download/"):
                    raise ValueError("Previous target feed is invalid")
                assets.append(carried)
    hashes.write_text("".join(f"{digest(f)}  {f.name}\n" for f in assets if f != hashes))
    # Adding Windows later must not change the already published Mac files. Expose its feed last.
    upload = [file for file in assets if not public or file != manifest]
    run(gh + ["release", "upload", tag, *[str(f.resolve()) for f in upload]] + ([] if public else ["--clobber"]))
    # Verify every staged asset over GitHub before making this draft the app's latest feed.
    verify = args.output / "github-verification"
    verify.mkdir(exist_ok=True)
    for file in upload:
        run(gh + ["release", "download", tag, "--pattern", file.name, "--dir", str(verify.resolve()), "--clobber"], stdout=subprocess.DEVNULL)
        if digest(file) != digest(verify / file.name):
            raise ValueError(f"GitHub asset hash mismatch: {file.name}")
    if public:
        run(gh + ["release", "upload", tag, str(manifest.resolve())])
        run(gh + ["release", "download", tag, "--pattern", manifest.name, "--dir", str(verify.resolve()), "--clobber"])
        if digest(manifest) != digest(verify / manifest.name):
            raise ValueError("Published target feed hash mismatch")
    else:
        run(gh + ["release", "edit", tag, "--draft=false", "--latest"])
    print(f"Published https://github.com/{REPOSITORY}/releases/tag/{tag}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
