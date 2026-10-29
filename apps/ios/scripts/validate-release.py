#!/usr/bin/env python3
"""Check the actual iOS app/archived bundle; never upload, resign, or change its contents."""
from pathlib import Path
import argparse
import json
import plistlib
import subprocess


def validate(bundle, require_distribution):
    archived = bundle.suffix == ".xcarchive"
    if archived:
        apps = list((bundle / "Products/Applications").glob("*.app"))
        if len(apps) != 1:
            raise SystemExit("Archive must contain exactly one application")
        bundle = apps[0]
    info = plistlib.loads((bundle / "Info.plist").read_bytes())
    manifest = plistlib.loads((bundle / "PrivacyInfo.xcprivacy").read_bytes())
    reasons = {entry["NSPrivacyAccessedAPIType"]: set(entry["NSPrivacyAccessedAPITypeReasons"])
               for entry in manifest["NSPrivacyAccessedAPITypes"]}
    # Native preferences and file metadata are app-owned; timers use clock_gettime, not mach_absolute_time.
    expected = {"NSPrivacyAccessedAPICategoryUserDefaults": {"CA92.1"},
                "NSPrivacyAccessedAPICategoryFileTimestamp": {"C617.1", "3B52.1"}}
    for category, values in expected.items():
        if not values.issubset(reasons.get(category, set())):
            raise SystemExit(f"Missing reviewed privacy API reason: {category}")
    if manifest["NSPrivacyTracking"] or manifest["NSPrivacyTrackingDomains"]:
        raise SystemExit("Unexpected tracking declaration; review actual data handling")
    root = Path(__file__).resolve().parents[1]
    # Check the packaged tables, not only source catalogs: stale Xcode resources can otherwise pass.
    locales = ["ko", "en", "ja", "zh-Hans", "zh-Hant", "de", "fr", "es", "pt-BR", "it"]
    permissions = json.loads((root / "Localization/Permissions.json").read_text())
    if info.get("CFBundleDevelopmentRegion") != "en":
        raise SystemExit("Expected English fallback for unsupported app languages")
    for locale in locales:
        expected_tables = {"Localizable.strings": json.loads((root / f"Localization/{locale}.json").read_text()),
                           "InfoPlist.strings": permissions[locale]}
        for name, expected_values in expected_tables.items():
            table = bundle / f"{locale}.lproj" / name
            actual = json.loads(subprocess.check_output(["plutil", "-convert", "json", "-o", "-", str(table)]))
            if actual != expected_values:
                raise SystemExit(f"Packaged localization differs from source: {locale}/{name}")
    source = root / "FTA/Legal"
    for name in ["PrivacyPolicy.txt", "PrivacyPolicy-en.txt", "ThirdPartyNotices.txt", "ThirdPartyNotices.json"]:
        if (source / name).read_bytes() != (bundle / name).read_bytes():
            raise SystemExit(f"Bundled document differs from reviewed source: {name}")
    if "iphoneos" not in info.get("CFBundleSupportedPlatforms", []):
        # Apple's actual platform spelling is iPhoneOS, while some tools normalize it to lowercase.
        if not any(str(p).lower() == "iphoneos" for p in info.get("CFBundleSupportedPlatforms", [])):
            raise SystemExit("Expected a physical-device iOS bundle")
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)
    signed = subprocess.run(["codesign", "-d", "--entitlements", ":-", str(bundle)], capture_output=True, check=True)
    entitlements = plistlib.loads(signed.stdout)
    development = entitlements.get("get-task-allow", False)
    if require_distribution and development:
        raise SystemExit("Development signing is not an App Store distribution export")
    if archived:
        # System SQLite is intentional on iOS; a feature-unification regression must not reintroduce
        # bundled SQLite's filesystem probes under a made-up available-disk-space reason.
        executable = bundle / info["CFBundleExecutable"]
        imports = subprocess.check_output(["nm", "-u", str(executable)], text=True)
        if any(symbol in imports for symbol in ["_statfs", "_fstatfs", "_statvfs", "_fstatvfs"]):
            raise SystemExit("Unexpected disk-space API import; audit dependency behavior before release")
        if "libsqlite3" not in subprocess.check_output(["otool", "-L", str(executable)], text=True):
            raise SystemExit("Expected the iOS system SQLite library")
        if "_mach_absolute_time" in imports and "35F9.1" not in reasons.get("NSPrivacyAccessedAPICategorySystemBootTime", set()):
            raise SystemExit("New system-boot-time API import requires a reviewed privacy reason")
    print(f"Validated iOS {info['CFBundleShortVersionString']}({info['CFBundleVersion']}), bundled privacy/notices, strict signature; development entitlement={development}")
    print("This local check does not replace App Store Connect processing/validation or export-compliance answers.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--require-distribution", action="store_true")
    arguments = parser.parse_args()
    validate(arguments.bundle, arguments.require_distribution)
