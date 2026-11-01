#!/usr/bin/env python3
"""Build a local signed Play candidate without exposing the upload password or touching installed phones."""
import argparse,hashlib,json,os,shutil,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];WORKSPACE=ROOT.parents[1]
if not os.environ.get('JAVA_HOME'):raise SystemExit('Set JAVA_HOME to the installed JDK.')
private=Path.home()/'.config/fta/android-signing';store=private/'upload.p12';helper=private/'upload-keychain'
if not store.is_file() or store.is_symlink() or not helper.is_file():raise SystemExit('Prepare or restore the approved upload candidate first; no debug key fallback is allowed.')
if store.stat().st_mode&0o077:raise SystemExit('Upload keystore permissions must be 0600.')
password=subprocess.check_output([str(helper),'read'],stderr=subprocess.PIPE).decode();env=os.environ.copy();env['FTA_ANDROID_UPLOAD_KEYSTORE']=str(store);env['FTA_ANDROID_UPLOAD_STORE_PASSWORD']=password
parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,default=WORKSPACE/'output/android-release');args=parser.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
# No daemon/configuration cache/build scan can retain the signing environment after this local build.
with (out/'release-build.log').open('wb') as log:
 result=subprocess.run([str(ROOT/'gradlew'),'-p',str(ROOT),'--no-daemon','--no-configuration-cache','--console=plain','--warn','bundleRelease','assembleRelease','lintRelease'],env=env,stdout=log,stderr=subprocess.STDOUT)
if result.returncode:raise SystemExit('Release build failed; inspect release-build.log (credentials are not printed).')
info=json.loads((ROOT/'app/build/outputs/apk/release/output-metadata.json').read_text())['elements'][0];prefix=f"FTA-{info['versionName']}-{info['versionCode']}"
files={prefix+'-release.aab':ROOT/'app/build/outputs/bundle/release/app-release.aab',prefix+'-release-arm64.apk':ROOT/'app/build/outputs/apk/release/app-release.apk'};metadata={}
for name,source in files.items():
 target=out/name;shutil.copy2(source,target);metadata[name]={'bytes':target.stat().st_size,'sha256':hashlib.sha256(target.read_bytes()).hexdigest()}
(out/'SHA256SUMS').write_text(''.join(f'{v["sha256"]}  {k}\n' for k,v in metadata.items()));(out/'build-artifacts.json').write_text(json.dumps(metadata,indent=2));print('Signed local release AAB/APK ready. No Play upload, submission or device installation was performed.')
