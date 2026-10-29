#!/usr/bin/env python3
"""Bundle exact existing dependency notices, without installing tools or downloading code."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
IOS = ROOT / "apps/ios"
OUTPUT = IOS / "FTA/Legal/ThirdPartyNotices.txt"
INDEX = IOS / "FTA/Legal/ThirdPartyNotices.json"
TARGET = "aarch64-apple-ios"


def cargo(*arguments):
    return subprocess.check_output(["cargo", *arguments], cwd=ROOT, text=True)


def generate():
    # Use the selected package/target graph, not other desktop/Android workspace dependencies.
    metadata = json.loads(cargo("metadata", "--offline", "--locked", "--filter-platform", TARGET, "--format-version", "1"))
    packages = {(p["name"], p["version"]): p for p in metadata["packages"]}
    tree = cargo("tree", "--offline", "--locked", "-p", "fta-ios", "--target", TARGET,
                 "-e", "normal,build", "--prefix", "none", "--no-dedupe", "--format", "{p}")
    selected = {match.groups() for line in tree.splitlines() if (match := re.match(r"([^ ]+) v([^ ]+)", line))}
    texts = {}
    entries = []
    index = []
    for key in sorted(selected):
        package = packages[key]
        if package["source"] is None:
            continue
        directory = Path(package["manifest_path"]).parent
        files = [p for p in directory.iterdir() if p.is_file() and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYRIGHT", "NOTICE", "COPYING"))]
        # These vendored sources have additional notices separate from the crate's top-level license.
        if package["name"] == "ring":
            files += list((directory / "third_party/fiat").glob("LICENSE*"))
            files += list((directory / "src/polyfill/once_cell").glob("LICENSE*"))
        if package["name"] == "tracing-core":
            files += list((directory / "src/spin").glob("LICENSE*"))
        source_note = ""
        if not files:
            # The published asn1-rs-impl archive omits its parent-repository license files.
            # Checked-in notices come from the exact .cargo_vcs_info commit and are hash verified.
            directory = IOS / "LegalSources" / f"{key[0]}-{key[1]}"
            provenance = json.loads((directory / "provenance.json").read_text())
            files = [directory / name for name in sorted(provenance)]
            for file in files:
                if hashlib.sha256(file.read_bytes()).hexdigest() != provenance[file.name]["sha256"]:
                    raise RuntimeError(f"Changed upstream notice: {file.name}")
            source_note = "\nUpstream notice sources: " + ", ".join(p["url"] for p in provenance.values())
        if not package["license"] or not files:
            raise RuntimeError(f"Missing license review for {key}")
        references = []
        linked_texts = []
        for file in sorted(set(files)):
            text = file.read_text(encoding="utf-8")
            digest = hashlib.sha256(text.encode()).hexdigest()
            texts[digest] = text
            references.append(f"  {file.relative_to(directory)}: {digest}")
            linked_texts.append({"file": str(file.relative_to(directory)), "digest": digest})
        entries.append(f"{key[0]} {key[1]}\nLicense expression: {package['license']}\nSource: {package['repository'] or package['source']}{source_note}\n" + "\n".join(references))
        index.append({"name": key[0], "version": key[1], "license": package["license"],
                      "source": package["repository"] or package["source"], "notices": linked_texts})
    # Cargo omits sysroot libraries: retain the matching toolchain's runtime copyrights as well.
    compiler = dict(line.split(": ", 1) for line in subprocess.check_output(
        ["rustc", "--version", "--verbose"], text=True).splitlines() if ": " in line)
    runtime = IOS / "LegalSources" / f"rust-runtime-{compiler['release']}"
    provenance = json.loads((runtime / "provenance.json").read_text())
    if compiler["commit-hash"] != provenance["commit"]:
        raise RuntimeError("Rust runtime attribution does not match this compiler; refresh/review runtime notices")
    references = []
    linked_texts = []
    for name, expected in provenance["files"].items():
        text = (runtime / name).read_text()
        digest = hashlib.sha256(text.encode()).hexdigest()
        if digest != expected:
            raise RuntimeError(f"Runtime notice hash mismatch: {name}")
        texts[digest] = text
        references.append(f"  {name}: {digest}")
        linked_texts.append({"file": name, "digest": digest})
    source = "https://github.com/rust-lang/rust/tree/" + provenance["commit"] + "/library"
    entries.append(f"Rust standard libraries {compiler['release']}\nSource: {source}\n" + "\n".join(references))
    index.append({"name": "Rust standard libraries", "version": compiler["release"],
                  "license": "Multiple licenses; see reproduced runtime notices", "source": source,
                  "notices": linked_texts})
    header = ("FTA iOS — Third-party notices\n\n"
              "This inventory covers existing normal and build-time dependencies of fta-ios for aarch64-apple-ios.\n"
              "Rust sysroot attribution is separately included because Cargo does not inventory standard libraries.\n"
              "Build tools/procedural macros and optional source notices may be listed even when their code is not shipped.\n"
              "License texts are reproduced unchanged; identical texts share their SHA-256 reference.\n"
              "No dependency implementation source is included in this notice document.\n\n")
    document = header + "\n\n".join(entries) + "\n\nLICENSE TEXTS\n\n" + "\n\n".join(f"SHA-256: {digest}\n{text}" for digest, text in sorted(texts.items())) + "\n"
    # Index by dependency so accessibility never has to traverse the entire 400KB notice document.
    return document, json.dumps({"packages": index, "texts": texts}, ensure_ascii=False, indent=2) + "\n"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Reject stale notices without changing source files")
    arguments = parser.parse_args()
    result, index = generate()
    if arguments.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != result or not INDEX.exists() or INDEX.read_text() != index:
            raise SystemExit("iOS dependency notices are stale; run apps/ios/scripts/generate-notices.py and review them")
        print("iOS dependency notices match the locked target build graph")
    else:
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT.write_text(result)
        INDEX.write_text(index)
        print(f"Wrote {OUTPUT.relative_to(ROOT)} ({len(result.encode())} bytes)")
