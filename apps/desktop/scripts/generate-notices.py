#!/usr/bin/env python3
"""Reproduce upstream notices for both desktop targets, including the official updater."""
import argparse
import importlib.util
import json
import hashlib
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUTPUT = ROOT / "apps/desktop/src-tauri/resources/THIRD_PARTY_NOTICES.txt"


def generate():
    # Share the existing source/hash-checked license inventory, including Rust runtime and ring notices.
    # Only graph selection changes; mobile notices and mobile application resources remain untouched.
    spec = importlib.util.spec_from_file_location("fta_license_inventory", ROOT / "apps/ios/scripts/generate-notices.py")
    source = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(source)
    packages, texts = {}, {}
    metadata = json.loads(source.cargo("metadata", "--locked", "--offline", "--format-version", "1"))
    by_name = {(p["name"], p["version"]): p for p in metadata["packages"]}
    selected = set()
    for target in ("aarch64-apple-darwin", "x86_64-pc-windows-msvc"):
        tree = source.cargo("tree", "--locked", "--offline", "-p", "fta-desktop", "--target", target,
                            "-e", "normal,build", "--prefix", "none", "--format", "{p}")
        selected.update(match.groups() for line in tree.splitlines() if (match := re.match(r"([^ ]+) v([^ ]+)", line)))
    for key in sorted(selected):
        package = by_name[key]
        if package["source"] is None:
            continue
        directory = Path(package["manifest_path"]).parent
        files = [p for p in directory.iterdir() if p.is_file() and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYRIGHT", "NOTICE", "COPYING"))]
        if package["name"] == "ring":
            files += list((directory / "third_party/fiat").glob("LICENSE*"))
            files += list((directory / "src/polyfill/once_cell").glob("LICENSE*"))
        if package["name"] == "tracing-core":
            files += list((directory / "src/spin").glob("LICENSE*"))
        if not files:
            # Some published crates omit parent-workspace notices. Preserve only exact-commit license texts,
            # with URL/hash provenance; implementation source is neither copied nor bundled here.
            candidates = [ROOT / "apps/desktop/LegalSources", ROOT / "apps/ios/LegalSources"]
            directory = next((base / f"{key[0]}-{key[1]}" for base in candidates if (base / f"{key[0]}-{key[1]}" / "provenance.json").exists()), None)
            if directory is None:
                raise RuntimeError(f"Missing upstream notice provenance for {key}")
            provenance = json.loads((directory / "provenance.json").read_text())
            files = [directory / name for name in provenance]
            for file in files:
                if hashlib.sha256(file.read_bytes()).hexdigest() != provenance[file.name]["sha256"]:
                    raise RuntimeError(f"Upstream notice hash changed: {file.name}")
        if not package["license"]:
            raise RuntimeError(f"Missing license expression for {key}")
        notices = []
        for file in sorted(set(files)):
            text = file.read_text()
            digest = hashlib.sha256(text.encode()).hexdigest()
            texts[digest] = text
            notices.append({"file": str(file.relative_to(directory)), "digest": digest})
        packages[key] = {"name": key[0], "version": key[1], "license": package["license"], "source": package["repository"] or package["source"], "notices": notices}
    # The existing inventory also verifies that runtime notices match the pinned compiler commit.
    _, index = source.generate()
    inventory = json.loads(index)
    runtime = next(p for p in inventory["packages"] if p["name"] == "Rust standard libraries")
    packages[runtime["name"], runtime["version"]] = runtime
    for notice in runtime["notices"]:
        texts[notice["digest"]] = inventory["texts"][notice["digest"]]
    sections = []
    for _, package in sorted(packages.items()):
        files = "\n".join(f"  {p['file']}: {p['digest']}" for p in package["notices"])
        source_archive = (f"\nUnmodified package source: https://crates.io/api/v1/crates/{package['name']}/{package['version']}/download"
                          if package["name"] != "Rust standard libraries" else "")
        sections.append(f"{package['name']} {package['version']}\nLicense: {package['license']}\nSource: {package['source']}{source_archive}\n{files}")
    return ("FTA desktop — Third-party notices\n\n"
            "Normal/build dependencies for Apple Silicon macOS and Windows x64; includes build tools and optional notices.\n"
            "Rust runtime attribution is included separately. This list is not a claim every listed package is linked.\n"
            "Original license/copyright texts are unchanged and deduplicated by SHA-256.\n\n"
            + "\n\n".join(sections) + "\n\nLICENSE TEXTS\n\n"
            + "\n\n".join(f"SHA-256: {digest}\n{text}" for digest, text in sorted(texts.items())) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    text = generate()
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != text:
            raise SystemExit("Desktop dependency notices are stale")
        print("Desktop dependency notices match both locked target graphs")
    else:
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT.write_text(text)
        print(f"Wrote desktop dependency notices: {len(text.encode())} bytes")
