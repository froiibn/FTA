#!/usr/bin/env python3
"""Exercise real TLS through adb forwards; no pretend online device enters the product UI."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[3]
SDK = Path(os.environ.get('ANDROID_HOME', Path.home() / 'Library/Android/sdk'))
ADB = [str(SDK / 'platform-tools/adb')]
if os.environ.get('ANDROID_SERIAL'):
    ADB += ['-s', os.environ['ANDROID_SERIAL']]

def adb(*args, timeout=20):
    return subprocess.check_output(ADB + list(args), timeout=timeout, stderr=subprocess.STDOUT)

if adb('shell','getprop','ro.kernel.qemu').strip()!=b'1':raise SystemExit('This fixture requires a dedicated Android emulator')
# Instrumentation must own a fresh process; terminating an old adb client alone does not stop its runner.
adb('shell','am','force-stop','app.fta.android')
(ROOT/'output').mkdir(exist_ok=True)
output = Path(tempfile.mkdtemp(prefix='android-background-', dir=ROOT / 'output'))
# A failed earlier run must not make the next run consume old bound ports.
adb('shell', 'run-as', 'app.fta.android', 'rm', '-f', 'files/background-endpoints.json')
log = (output / 'instrumentation.log').open('wb')
adb('shell','pm','grant','app.fta.android','android.permission.ACCESS_LOCAL_NETWORK')
adb('shell','pm','grant','app.fta.android','android.permission.POST_NOTIFICATIONS')
run = subprocess.Popen(ADB + ['shell', 'am', 'instrument', '-w', '-e','background','true','app.fta.android.test/app.fta.android.FtaChecks'], stdout=log, stderr=subprocess.STDOUT)
forwarded, reversed_ports = [], []
fixture = None
success = False
try:
    start = time.monotonic()
    while True:
        try:
            native = json.loads(adb('exec-out', 'run-as', 'app.fta.android', 'cat', 'files/background-endpoints.json', timeout=5))
            break
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired, json.JSONDecodeError):
            if run.poll() is not None or time.monotonic() - start > 50:
                raise RuntimeError('Android listener did not publish verification endpoints')
            time.sleep(0.2)
    for key in ('address', 'pairing_address'):
        port = native[key].rsplit(':', 1)[1]
        adb('forward', 'tcp:' + port, 'tcp:' + port)
        forwarded.append(port)
    endpoints = output / 'android-endpoints.json'
    endpoints.write_text(json.dumps(native))
    fixture = subprocess.Popen([str(ROOT / 'target/debug/examples/transport_fixture'), str(output), str(endpoints)],
        stdout=(output / 'mac.log').open('wb'), stderr=subprocess.STDOUT)
    start = time.monotonic()
    while not (output / 'mac-endpoints.json').exists():
        if fixture.poll() is not None or time.monotonic() - start > 20:
            raise RuntimeError('Mac fixture did not bind its listener')
        time.sleep(0.1)
    peer = json.loads((output / 'mac-endpoints.json').read_text())
    # The emulator's documented host-loopback alias reaches this real Mac listener; only TLS bytes are proxied inbound.
    temporary='/data/local/tmp/fta-background-mac.json'
    adb('push',str(output/'mac-endpoints.json'),temporary)
    adb('shell','run-as','app.fta.android','cp',temporary,'files/background-mac.json')
    adb('shell','rm','-f',temporary)
    (output / 'reverse-ready').touch()
    if fixture.wait(timeout=240) != 0:
        raise RuntimeError((output / 'mac.log').read_text())
    run.wait(timeout=90)
    log.flush()
    android_result = (output / 'instrumentation.log').read_text()
    if 'FTA background Service checks passed:' not in android_result:
        raise RuntimeError(android_result)
    print((output / 'mac.log').read_text().strip())
    print(android_result.strip())
    print('Verification artifacts:', output)
    success = True
finally:
    if not success:
        adb('shell','am','force-stop','app.fta.android')
    if run.poll() is None:
        run.terminate()
    if fixture and fixture.poll() is None:
        fixture.terminate()
    for port in forwarded:
        adb('forward', '--remove', 'tcp:' + port)
    for port in reversed_ports:
        adb('reverse', '--remove', 'tcp:' + port)
    log.close()
