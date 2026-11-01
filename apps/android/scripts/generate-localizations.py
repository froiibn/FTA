#!/usr/bin/env python3
"""Generate SDK resources from Android-owned catalogs; never rewrite protocol or preference keys."""
import argparse,html,json,re
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
LOCALES={'en':'values','ko':'values-ko','ja':'values-ja','zh-Hans':'values-b+zh+Hans','zh-Hant':'values-b+zh+Hant','de':'values-de','fr':'values-fr','es':'values-es','pt-BR':'values-pt-rBR','it':'values-it'}
parser=argparse.ArgumentParser();parser.add_argument('--check',action='store_true');args=parser.parse_args()
base=json.loads((ROOT/'Localization/en.json').read_text());quantities=json.loads((ROOT/'Localization/plurals.json').read_text())
def escaped(text):
    return '"'+html.escape(text,quote=False).replace('\\','\\\\').replace('\n','\\n').replace("'","\\'").replace('"','\\"')+'"'
def formats(text):return sorted(re.findall(r'%(?:\d+\$)?[sdf]',text))
for locale,folder in LOCALES.items():
    strings=json.loads((ROOT/f'Localization/{locale}.json').read_text());assert strings.keys()==base.keys(),locale
    lines=['<resources>','    <!-- Generated from Android catalogs: locale changes must not mutate stored data or network metadata. -->']
    for key,value in strings.items():
        assert value and formats(value)==formats(base[key]),(locale,key,'format mismatch')
        assert locale=='ko' or not re.search('[가-힣]',value),(locale,key,'Korean leakage')
        lines.append(f'    <string name="{key}">{escaped(value)}</string>')
    for key,forms in quantities[locale].items():
        lines.append(f'    <plurals name="{key}">')
        for quantity,text in forms.items():lines.append(f'        <item quantity="{quantity}">{escaped(text)}</item>')
        lines.append('    </plurals>')
    lines.append('</resources>');text='\n'.join(lines)+'\n';path=ROOT/f'app/src/main/res/{folder}/strings.xml'
    if args.check:assert path.read_text()==text,f'stale generated resources: {path}'
    else:path.parent.mkdir(exist_ok=True);path.write_text(text)
print(f'10 locales, {len(base)} strings and 3 quantity resources verified')
