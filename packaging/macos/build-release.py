#!/usr/bin/env python3
"""Local-only macOS checks, Developer ID packaging, notarization and optional GitHub publication."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--identity", default=os.environ.get("APPLE_SIGNING_IDENTITY"), required=not os.environ.get("APPLE_SIGNING_IDENTITY"))
    parser.add_argument("--notary-profile", default=os.environ.get("FTA_NOTARY_PROFILE"), required=not os.environ.get("FTA_NOTARY_PROFILE"))
    parser.add_argument("--publish", action="store_true")
    parser.add_argument("--key", type=Path, default=Path.home() / ".config/fta/update-signing/desktop.key")
    parser.add_argument("--gh", default=shutil.which("gh"))
    args = parser.parse_args()
    if sys.platform != "darwin":
        sys.exit("Build and verify macOS releases on the owner's Mac")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip():
        sys.exit("Commit the source before building the release")
    config = json.loads((ROOT / "apps/desktop/src-tauri/tauri.conf.json").read_text())
    version = config["version"]
    arch = "aarch64" if platform.machine() == "arm64" else "x86_64"
    target = "darwin-" + arch
    output = ROOT / "output" / f"release-macos-{version}"
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, APPLE_SIGNING_IDENTITY=args.identity, CI="true")
    cargo_home = Path(env.get("CARGO_HOME", Path.home() / ".cargo"))
    env["PATH"] = str(cargo_home / "bin") + os.pathsep + env["PATH"]
    # Accept a key file path only; do not place private key contents in command arguments or build logs.
    env["TAURI_SIGNING_PRIVATE_KEY"] = str(args.key.resolve())
    env.setdefault("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "")
    evidence = []

    def checked(command, name):
        with (output / (name + ".log")).open("w") as log:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        evidence.append({"command": command, "exit_code": result.returncode, "log": name + ".log"})
        if result.returncode:
            sys.exit(f"Failed {name}; see {output / (name + '.log')}")
        print(f"Passed {name}", flush=True)

    commands = [
        ["cargo", "fetch", "--locked"],
        [sys.executable, "apps/desktop/scripts/generate-notices.py", "--check"],
        ["cargo", "fmt", "--all", "--check"], ["cargo", "test", "--locked"],
        ["cargo", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"],
        ["npm", "--prefix", "apps/desktop", "run", "build"],
        ["cargo", "check", "--locked", "-p", "fta-desktop"],
        ["cargo", "test", "--locked", "-p", "fta-desktop"],
        ["cargo", "clippy", "--locked", "-p", "fta-desktop", "--all-targets", "--", "-D", "warnings"],
        ["npm", "--prefix", "apps/desktop", "run", "test:feedback"],
        ["cargo", "build", "--locked", "--release", "-p", "fta-cli"],
        ["npm", "--prefix", "apps/desktop", "run", "bundle", "--", "--bundles", "app,dmg"],
    ]
    for index, command in enumerate(commands):
        checked(command, f"build-check-{index}")
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=ROOT, env=env, text=True))
    release = Path(metadata["target_directory"]) / "release/bundle"
    app = release / "macos/FTA.app"
    dmg = release / "dmg" / f"FTA_{version}_{arch}.dmg"
    checked(["codesign", "--verify", "--deep", "--strict", str(app)], "signature-app")
    checked(["codesign", "--verify", "--strict", str(dmg)], "signature-dmg")
    # Notarize the signed image first; stamp both app and image before producing the final update archive.
    notary = subprocess.run(["xcrun", "notarytool", "submit", str(dmg), "--keychain-profile", args.notary_profile,
                             "--wait", "--output-format", "json"], cwd=ROOT, capture_output=True, text=True)
    (output / "notary-submit.json").write_text(notary.stdout)
    if notary.returncode or json.loads(notary.stdout).get("status") != "Accepted":
        sys.exit("Apple notarization was not Accepted; nothing was published")
    result = json.loads(notary.stdout)
    for name, path in [("app", app), ("dmg", dmg)]:
        checked(["xcrun", "stapler", "staple", str(path)], "staple-" + name)
        checked(["xcrun", "stapler", "validate", str(path)], "ticket-" + name)
    checked(["spctl", "--assess", "--type", "execute", "--verbose=2", str(app)], "gatekeeper-app")
    checked(["spctl", "--assess", "--type", "open", "--context", "context:primary-signature", "--verbose=2", str(dmg)], "gatekeeper-dmg")
    checked(["hdiutil", "verify", str(dmg)], "dmg-integrity")
    installer = output / dmg.name
    shutil.copy2(dmg, installer)
    artifact = output / f"FTA_{version}_{arch}.app.tar.gz"
    # Omit AppleDouble metadata but retain bundle files, executable modes and the stapled app ticket.
    with artifact.open("wb") as stream:
        subprocess.run(["tar", "-czf", "-", "-C", str(app.parent), app.name], env=dict(env, COPYFILE_DISABLE="1"), stdout=stream, check=True)
    info = output / f"build-info-{target}-{version}.json"
    info.write_text(json.dumps({
        "version": version, "target": target,
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "built_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "native_checks_passed": True, "native_os": platform.platform(),
        "artifact_sha256": sha(artifact), "installer_sha256": sha(installer),
        "notarization": {"accepted": True, "id": result["id"], "app_ticket": True, "dmg_ticket": True},
        "checks": evidence, "windows_native_verification": False,
    }, indent=2) + "\n")
    publish = [sys.executable, str(ROOT / "packaging/publish-release.py"), "--target", target, "--artifact", str(artifact),
               "--installer", str(installer), "--build-info", str(info), "--notes", str(ROOT / f"docs/releases/{version}.md"), "--output", str(output / "assets"), "--key", str(args.key.resolve())]
    if args.gh:
        publish += ["--gh", args.gh]
    if args.publish:
        publish += ["--publish"]
    checked(publish, "release-publication")
    if args.publish:
        # Exercise the actual public download, signature rejection and installer against a disposable app only.
        checked(["cargo", "test", "--locked", "-p", "fta-desktop", "published_mac_update_installs_only_into_isolated_app", "--", "--ignored", "--nocapture"], "published-update-install")
    print(output, flush=True)


if __name__ == "__main__":
    main()
