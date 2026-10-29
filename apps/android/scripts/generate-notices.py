#!/usr/bin/env python3
"""Package unchanged license texts from the locked Android Rust graph, without downloading source."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT=Path(__file__).resolve().parents[3]
APP=ROOT/'apps/android'
TARGET='aarch64-linux-android'
OUT=APP/'app/src/main/assets/ThirdPartyNotices.txt'
INDEX=APP/'app/src/main/assets/ThirdPartyNotices.json'

def cargo(*args):return subprocess.check_output(['cargo',*args],cwd=ROOT,text=True)
def generate():
    metadata=json.loads(cargo('metadata','--offline','--locked','--filter-platform',TARGET,'--format-version','1'))
    packages={(p['name'],p['version']):p for p in metadata['packages']}
    tree=cargo('tree','--offline','--locked','-p','fta-android','--target',TARGET,'-e','normal,build','--prefix','none','--no-dedupe','--format','{p}')
    selected={m.groups() for line in tree.splitlines() if (m:=re.match(r'([^ ]+) v([^ ]+)',line))}
    sections=[];texts={};index=[]
    for name,version in sorted(selected):
        package=packages[(name,version)]
        if package['source'] is None:continue
        folder=Path(package['manifest_path']).parent
        files=[p for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE','LICENCE','COPYRIGHT','NOTICE','COPYING'))]
        if name=='ring':
            files+=list((folder/'third_party/fiat').glob('LICENSE*'))
            files+=list((folder/'src/polyfill/once_cell').glob('LICENSE*'))
        if name=='tracing-core':files+=list((folder/'src/spin').glob('LICENSE*'))
        # This upstream crate omits license files from its archive; reuse the existing project's reviewed copies.
        if not files and name=='asn1-rs-impl':files=list((APP/f'LegalSources/{name}-{version}').glob('LICENSE*'))
        if not files:raise RuntimeError(f'License texts missing for {name} {version}')
        refs=[]
        for file in sorted(files):
            text=file.read_text(errors='strict');digest=hashlib.sha256(text.encode()).hexdigest();texts[digest]=text
            refs.append({'file':file.name,'sha256':digest})
        index.append({'name':name,'version':version,'license':package['license'],'source':package['repository'] or package['source'],'notices':refs})
        sections.append(f"{name} {version}\nLicense: {package['license']}\nSource: {package['repository'] or package['source']}\n"+'\n'.join(f"{r['file']}: {r['sha256']}" for r in refs))
    text='FTA Android — Third-party notices\n\nExisting locked normal/build Rust dependencies for aarch64-linux-android. Build-only tools may be listed even when their code is not shipped. Identical license texts share a SHA-256 reference.\n\n'+'\n\n'.join(sections)+'\n\nLICENSE TEXTS\n\n'+'\n\n'.join(f'SHA-256: {key}\n{value}' for key,value in sorted(texts.items()))+'\n'
    return text,json.dumps(index,ensure_ascii=False,indent=2)+'\n'
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--check',action='store_true');args=parser.parse_args()
    text,index=generate()
    if args.check:
        if not OUT.exists() or OUT.read_text()!=text or not INDEX.exists() or INDEX.read_text()!=index:raise SystemExit('Android notices are stale; regenerate and review them')
        print('Android notices match the locked target graph')
    else:
        OUT.parent.mkdir(parents=True,exist_ok=True);OUT.write_text(text);INDEX.write_text(index);print('Wrote Android notices',len(text.encode()),'bytes')
