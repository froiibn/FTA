#!/usr/bin/env python3
"""Native Android controls against the real app and emulator network; never supplies fake online peers."""
import json
import os
from pathlib import Path
import re
import subprocess
import time
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[3]
SDK=Path(os.environ.get('ANDROID_HOME',Path.home()/'Library/Android/sdk'))
ADB=[str(SDK/'platform-tools/adb')]
if os.environ.get('ANDROID_SERIAL'):ADB+=['-s',os.environ['ANDROID_SERIAL']]
OUT=ROOT/'output/android-ui-checks';OUT.mkdir(parents=True,exist_ok=True)
def adb(*args,timeout=20):return subprocess.check_output(ADB+list(args),timeout=timeout,stderr=subprocess.STDOUT)
def tree():
 for attempt in range(4):
  try:
   adb('shell','rm','-f','/sdcard/fta-smoke.xml')
   adb('shell','uiautomator','dump','--compressed','/sdcard/fta-smoke.xml')
   data=adb('exec-out','cat','/sdcard/fta-smoke.xml');(OUT/'latest.xml').write_bytes(data)
   return ET.fromstring(data)
  except (ET.ParseError,subprocess.SubprocessError):time.sleep(.5)
 raise RuntimeError('No stable accessibility root')
def find(text):return next((n for n in tree().iter('node') if n.get('text')==text or n.get('hint')==text),None)
def wait(text,timeout=20):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  node=find(text)
  if node is not None:return node
  time.sleep(.2)
 raise AssertionError('Missing native control: '+text)
def tap_node(node):
 box=list(map(int,re.findall(r'\d+',node.get('bounds'))));adb('shell','input','tap',str((box[0]+box[2])//2),str((box[1]+box[3])//2))
def tap(text):tap_node(wait(text))
def launch():adb('shell','am','start','-W','-n','app.fta.android/.MainActivity',timeout=55)
def screen(name):(OUT/(name+'.png')).write_bytes(adb('exec-out','screencap','-p'))
def main():
    assert adb('shell','getprop','ro.kernel.qemu').strip()==b'1','This test requires a dedicated Android emulator'
    launch();tap('보내기');wait('텍스트 또는 URL');tap('설정')
    trust=wait('신뢰하는 집 네트워크입니다')
    assert trust.get('enabled')=='true','Emulator must have a real Wi-Fi LAN'
    if trust.get('checked')!='true':tap_node(trust)
    wait('이 화면이 열려 있는 동안 수신 가능');screen('settings-online')
    config=json.loads(adb('exec-out','run-as','app.fta.android','cat','no_backup/engine/config.json'));identity=config['device_id']
    adb('shell','am','force-stop','app.fta.android');launch();wait('이 화면이 열려 있는 동안 수신 가능');tap('설정');assert wait('신뢰하는 집 네트워크입니다').get('checked')=='true'
    tap('송수신 사용');wait('송수신 중지됨');adb('shell','am','force-stop','app.fta.android');launch();wait('송수신 중지됨');tap('설정');assert wait('송수신 사용').get('checked')=='false'
    tap('송수신 사용');wait('이 화면이 열려 있는 동안 수신 가능')
    try:
     adb('shell','svc','wifi','disable');wait('LAN 연결 없음');screen('wifi-disconnected')
    finally:adb('shell','svc','wifi','enable')
    wait('이 화면이 열려 있는 동안 수신 가능',timeout=40);assert wait('신뢰하는 집 네트워크입니다').get('checked')=='true'
    tap('기록');wait('전송 기록');screen('history');tap('보내기');wait('텍스트 또는 URL');screen('send')
    config=json.loads(adb('exec-out','run-as','app.fta.android','cat','no_backup/engine/config.json'));assert identity==config['device_id']
    tap('사진');wait('갤러리에서 선택');wait('촬영');screen('photo-source-menu');adb('shell','input','keyevent','4')
    print('Native UI passed: real LAN trust, process restart, receive toggle persistence, Wi-Fi disconnect/reconnect, retained identity, native tabs and photo source menu')
    print('UI artifacts:',OUT)

if __name__=='__main__':main()
