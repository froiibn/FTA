#!/usr/bin/env python3
"""Validate reviewed translations and generate native string tables without external dependencies."""
from pathlib import Path
import argparse
import json
import re

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "Localization"
OUTPUT = ROOT / "FTA/Localization"
LOCALES = ["ko", "en", "ja", "zh-Hans", "zh-Hant", "de", "fr", "es", "pt-BR", "it"]
UI_EXPECTATION_KEYS = ["보내기", "기록", "설정", "사진", "파일", "송수신 사용", "전송할 텍스트 또는 URL", "전송 취소", "개인정보 처리방침", "갤러리에서 선택", "촬영"]
PLACEHOLDER = re.compile(r"%(?:\d+\$)?(?:lld|ld|@|d|f|%)")

def table(values):
    # JSON quoting and Apple's .strings syntax agree for this UTF-8 literal/escape subset.
    return "// Generated from reviewed translations; edit apps/ios/Localization instead.\n" + "\n".join(
        f"{json.dumps(key,ensure_ascii=False)} = {json.dumps(value,ensure_ascii=False)};"
        for key,value in sorted(values.items())) + "\n"

def generate(check):
    keys = set(json.loads((SOURCE / "keys.json").read_text()))
    permissions = json.loads((SOURCE / "Permissions.json").read_text())
    expectations = {}
    for locale in LOCALES:
        values = json.loads((SOURCE / f"{locale}.json").read_text())
        if set(values) != keys:
            raise SystemExit(f"Missing/extra translations: {locale}")
        for key,value in values.items():
            if not value or sorted(PLACEHOLDER.findall(key)) != sorted(PLACEHOLDER.findall(value)):
                raise SystemExit(f"Empty text or mismatched format arguments: {locale}: {key}")
            if locale != "ko" and re.search(r"[가-힣]",value):
                raise SystemExit(f"Korean source copy leaked into {locale}: {key}")
        # Generate test labels from the same source so locale coverage cannot silently drift.
        expectations[locale] = {key: values[key] for key in UI_EXPECTATION_KEYS}
        for filename,content in [("Localizable.strings",table(values)),("InfoPlist.strings",table(permissions[locale]))]:
            destination = OUTPUT / f"{locale}.lproj" / filename
            if check:
                if not destination.exists() or destination.read_text() != content:
                    raise SystemExit(f"Stale native localization table: {destination}")
            else:
                destination.parent.mkdir(parents=True,exist_ok=True); destination.write_text(content)
    fixture = ROOT / "Tests/LocaleExpectations.json"
    if check:
        if not fixture.exists() or json.loads(fixture.read_text()) != expectations:
            raise SystemExit("Stale UI locale expectations")
    else:
        fixture.write_text(json.dumps(expectations,ensure_ascii=False,indent=2)+"\n")
    print(f"Validated {len(keys)} UI keys and 2 permission descriptions in {len(LOCALES)} locales")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("--check",action="store_true")
    generate(parser.parse_args().check)
