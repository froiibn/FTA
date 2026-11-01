#!/usr/bin/env python3
"""Prepare an isolated local Play upload candidate; never print, commit or rotate a private key/password."""
import hashlib,os,secrets,subprocess,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
if not os.environ.get('JAVA_HOME'):raise SystemExit('Set JAVA_HOME to the installed JDK before preparing an upload key.')
private=Path.home()/'.config/fta/android-signing';private.mkdir(parents=True,exist_ok=True);private.chmod(0o700)
helper=private/'upload-keychain';source=ROOT/'scripts/upload-keychain.swift'
if not helper.exists():
    subprocess.run(['/usr/bin/swiftc',str(source),'-o',str(helper)],check=True,capture_output=True)
    helper.chmod(0o700)
store=private/'upload.p12';probe=subprocess.run([str(helper),'read'],capture_output=True)
if store.exists():
    if probe.returncode:raise SystemExit('Existing upload keystore has no accessible Keychain password; restore access, do not rotate.')
    password=probe.stdout.decode()
else:
    if probe.returncode!=4:raise SystemExit('Keychain status requires review; no key was generated.')
    password=secrets.token_urlsafe(48);env=os.environ.copy();env['FTA_UPLOAD_PASSWORD']=password
    jdk=Path(os.environ['JAVA_HOME'])
    # Create a new file only; keytool must not overwrite any existing private signing material.
    temp=private/('candidate-'+secrets.token_hex(8)+'.p12')
    try:
        subprocess.run([str(jdk/'bin/keytool'),'-genkeypair','-keystore',str(temp),'-storetype','PKCS12','-alias','fta-upload','-keyalg','RSA','-keysize','3072','-sigalg','SHA256withRSA','-validity','10000','-dname','CN=FTA Android upload','-storepass:env','FTA_UPLOAD_PASSWORD','-keypass:env','FTA_UPLOAD_PASSWORD'],env=env,check=True,capture_output=True)
        temp.chmod(0o600);subprocess.run([str(helper),'store'],input=password.encode(),check=True,capture_output=True);os.replace(temp,store)
    finally:
        if temp.exists():temp.unlink()
    env.pop('FTA_UPLOAD_PASSWORD',None)
# Export only the public certificate; this is a local candidate until the owner configures Play App Signing.
jdk=Path(os.environ['JAVA_HOME']);env=os.environ.copy();env['FTA_UPLOAD_PASSWORD']=password
certificate=subprocess.check_output([str(jdk/'bin/keytool'),'-exportcert','-keystore',str(store),'-alias','fta-upload','-storepass:env','FTA_UPLOAD_PASSWORD'],env=env,stderr=subprocess.PIPE)
out=ROOT/'Store';out.mkdir(exist_ok=True);pem=subprocess.check_output([str(jdk/'bin/keytool'),'-exportcert','-rfc','-keystore',str(store),'-alias','fta-upload','-storepass:env','FTA_UPLOAD_PASSWORD'],env=env,stderr=subprocess.PIPE);(out/'upload-candidate-certificate.pem').write_bytes(pem)
print('Local upload candidate ready; private keystore stays outside the repository and password stays in OS Keychain.')
print('Public certificate SHA-256:',hashlib.sha256(certificate).hexdigest())
