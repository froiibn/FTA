#!/usr/bin/env python3
"""Inspect the real signed artifacts; private keystores/passwords are never inputs to this verifier."""
import argparse,hashlib,json,os,re,ssl,struct,subprocess,xml.etree.ElementTree as ET,zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];p=argparse.ArgumentParser();p.add_argument('directory',type=Path);args=p.parse_args();out=args.directory.resolve()
jdk=Path(os.environ['JAVA_HOME']);sdk=Path(os.environ.get('ANDROID_HOME',Path.home()/'Library/Android/sdk'));tools=sorted((sdk/'build-tools').iterdir())[-1]
aab=next(out.glob('FTA-*-release.aab'));apk=next(out.glob('FTA-*-release-arm64.apk'));ns='{http://schemas.android.com/apk/res/android}';manifest=ET.parse(out/'release-manifest.xml').getroot();app=manifest.find('application');api=manifest.find('uses-sdk')
report={'package':manifest.get('package'),'versionCode':manifest.get(ns+'versionCode'),'minSdk':api.get(ns+'minSdkVersion'),'targetSdk':api.get(ns+'targetSdkVersion'),'debuggable':app.get(ns+'debuggable','false'),'instrumentationAbsent':manifest.find('instrumentation') is None}
assert report['package']=='app.fta.android' and report['minSdk']=='29' and report['targetSdk']=='37' and report['debuggable']=='false' and report['instrumentationAbsent']
with zipfile.ZipFile(aab) as archive:
 names=archive.namelist();libraries=[x for x in names if x.endswith('.so')];report['abis']=sorted(set(x.split('/')[2] for x in libraries));assert report['abis']==['arm64-v8a']
 assert not any(x.endswith(('.p12','.jks','.keystore')) or 'gallery-check.heic' in x for x in names)
 dex=b''.join(archive.read(x) for x in names if x.endswith('.dex'));report['testClassesAbsent']=all(x not in dex for x in [b'Lapp/fta/android/FtaChecks;',b'Lapp/fta/android/GalleryChecks;']);assert report['testClassesAbsent']
 data=archive.read(libraries[0]);offset=struct.unpack_from('<Q',data,32)[0];size,count=struct.unpack_from('<HH',data,54);headers=[struct.unpack_from('<IIQQQQQQ',data,offset+i*size) for i in range(count)];report['elf16KiBAligned']=all(x[7]>=16384 and (x[3]-x[2])%16384==0 for x in headers if x[0]==1);assert report['elf16KiBAligned']
 shoff=struct.unpack_from('<Q',data,40)[0];shsize,shnum,stringIndex=struct.unpack_from('<HHH',data,58);sections=[struct.unpack_from('<IIQQQQIIQQ',data,shoff+i*shsize) for i in range(shnum)];strings=sections[stringIndex];table=data[strings[4]:strings[4]+strings[5]];sectionNames=[table[x[0]:].split(b'\0',1)[0].decode() for x in sections];report['nativeDebugSectionsAbsent']=not any(x.startswith('.debug') for x in sectionNames);assert report['nativeDebugSectionsAbsent']
 for file in ['PrivacyPolicy.txt','PrivacyPolicy.en.txt','ThirdPartyNotices.txt','ThirdPartyNotices.json']:assert archive.read('base/assets/'+file)==(ROOT/'app/src/main/assets'/file).read_bytes()
 report['legalAssetsMatchSource']=True
certificate=ssl.PEM_cert_to_DER_cert((ROOT/'Store/upload-candidate-certificate.pem').read_text());report['publicUploadCertificateSha256']=hashlib.sha256(certificate).hexdigest()
r=subprocess.run([str(tools/'apksigner'),'verify','--verbose','--print-certs',str(apk)],capture_output=True,check=True);(out/'apk-signature.txt').write_bytes(r.stdout+r.stderr);assert report['publicUploadCertificateSha256'] in r.stdout.decode();report['releaseApkMatchesUploadCertificate']=True
r=subprocess.run([str(jdk/'bin/jarsigner'),'-verify',str(aab)],capture_output=True,check=True);(out/'aab-signature.txt').write_bytes(r.stdout+r.stderr);report['aabSignatureVerified']='jar verified.' in r.stdout.decode();assert report['aabSignatureVerified']
r=subprocess.run([str(tools/'zipalign'),'-c','-P','16','-v','4',str(apk)],capture_output=True,check=True);(out/'apk-alignment.txt').write_bytes(r.stdout);report['apkZip16KiBAligned']=True
# Check each language value in the actual AAB resource table, not only the source XML or a preview.
resources=(out/'release-resources.txt').read_text();catalog=json.loads((ROOT/'Localization/en.json').read_text());expected={'ja','de','zh-Hans','zh-Hant','ko','fr','es','it','pt-BR'}
for key in catalog:
 match=re.search(r' - string/'+re.escape(key)+r'\n(.*?)(?=\n0x|\Z)',resources,re.S);assert match,key
 block=match.group(1);assert '(default) - [STR]' in block,key
 assert set(re.findall(r'locale: "([^"]+)"',block))==expected,key
report['fullyLocalizedStrings']=len(catalog);report['locales']=10
(out/'release-validation.json').write_text(json.dumps(report,indent=2));print('Signed AAB/APK manifest, certificate, optimized ELF,16KiB alignment,legal assets,test absence and all10 resource sets verified')
