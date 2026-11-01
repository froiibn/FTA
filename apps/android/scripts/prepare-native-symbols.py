#!/usr/bin/env python3
"""Create a Play symbol sidecar only if stripping reproduces the supplied AAB exactly."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument("aab", type=Path)
parser.add_argument("--output-dir", required=True, type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parents[3]
sdk = Path(os.environ.get("ANDROID_HOME", os.environ.get("ANDROID_SDK_ROOT", Path.home() / "Library/Android/sdk")))
ndk = Path(os.environ.get("ANDROID_NDK_HOME", sdk / "ndk/28.2.13676358"))
host = "darwin-x86_64" if sys.platform == "darwin" else "linux-x86_64"
tools = ndk / "toolchains/llvm/prebuilt" / host / "bin"
archive = root / "target/aarch64-linux-android/release/libfta_android.a"
if not archive.is_file():
    raise SystemExit("Build the locked Android release archive first; this script never rebuilds or changes the app.")
with zipfile.ZipFile(args.aab) as bundle:
    native_paths = [name for name in bundle.namelist() if name.endswith(".so")]
    if native_paths != ["base/lib/arm64-v8a/libfta_android.so"]:
        raise SystemExit("Unexpected native library set; refuse to claim partial symbol coverage.")
    packaged = bundle.read(native_paths[0])
args.output_dir.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix="fta-native-symbols-") as temporary:
    temporary = Path(temporary)
    original = temporary / "libfta_android.so"
    # Reuse the exact archive and linker flags of build-native.sh. Byte comparison below
    # rejects stale archives or future flag drift instead of attaching misleading symbols.
    subprocess.run([
        str(tools / "aarch64-linux-android29-clang"), "-std=c11", "-Wall", "-Wextra", "-Werror", "-fPIC", "-shared", "-O2", "-DNDEBUG",
        str(root / "apps/android/native/bridge.c"), str(archive), "-Wl,--gc-sections", "-Wl,--exclude-libs,ALL",
        "-Wl,--version-script," + str(root / "apps/android/native/exports.map"),
        "-Wl,-z,max-page-size=16384", "-Wl,-z,defs", "-ldl", "-llog", "-landroid", "-lm", "-o", str(original),
    ], check=True)
    stripped = temporary / "stripped.so"
    stripped.write_bytes(original.read_bytes())
    subprocess.run([str(tools / "llvm-strip"), "--strip-unneeded", str(stripped)], check=True)
    if stripped.read_bytes() != packaged:
        raise SystemExit("The stripped relink differs from the supplied AAB; no symbols were published.")
    sections = subprocess.check_output([str(tools / "llvm-readelf"), "-S", str(original)]).decode()
    if ".symtab" not in sections:
        raise SystemExit("The sidecar has no native symbol table.")
    destination = args.output_dir / "native-debug-symbols.zip"
    # A symbol-table sidecar is separate from the signed app. Never enable app debugging
    # or put test classes, private upload keys, or source/debug sections into the AAB.
    with zipfile.ZipFile(destination, "w", compression=zipfile.ZIP_DEFLATED) as output:
        output.write(original, "arm64-v8a/libfta_android.so")
    record = {
        "mode": "SYMBOL_TABLE", "strippedRelinkExactlyMatchesUploadedAab": True,
        "uploadedAabNativeSha256": hashlib.sha256(packaged).hexdigest(),
        "symbolArchiveSha256": hashlib.sha256(destination.read_bytes()).hexdigest(),
        "appBinaryChanged": False,
    }
    (args.output_dir / "native-symbol-validation.json").write_text(json.dumps(record, indent=2) + "\n")
print("Verified symbol sidecar:", destination)
