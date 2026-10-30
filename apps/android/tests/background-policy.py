#!/usr/bin/env python3
"""Exercise real OS stop/permission/foreground boundaries on the dedicated emulator; preserve saved identity/trust."""
import importlib.util,json,time
from pathlib import Path
spec=importlib.util.spec_from_file_location('ui',Path(__file__).with_name('ui-smoke.py'));ui=importlib.util.module_from_spec(spec);spec.loader.exec_module(ui)
assert ui.adb('shell','getprop','ro.kernel.qemu').strip()==b'1','Dedicated emulator only'
OUT=ui.ROOT/'output/android-background-policy';OUT.mkdir(exist_ok=True)
PKG='app.fta.android';checks=[]
def foreground():return b'isForeground=true' in ui.adb('shell','dumpsys','activity','services',PKG)
def wait_for(test,label,timeout=20):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  if test():checks.append(label);print(label,flush=True);return
  time.sleep(.2)
 raise AssertionError(label)
def launch():ui.adb('shell','input','keyevent','WAKEUP');ui.adb('shell','wm','dismiss-keyguard');ui.launch();ui.tap('보내기')
def identity():return json.loads(ui.adb('exec-out','run-as',PKG,'cat','no_backup/engine/config.json'))['device_id']
def screen(name):ui.tree();(OUT/(name+'.png')).write_bytes(ui.adb('exec-out','screencap','-p'))
package=ui.adb('shell','dumpsys','package',PKG).decode()
for permission in ['ACCESS_LOCAL_NETWORK','POST_NOTIFICATIONS']:
 if 'android.permission.'+permission+': granted=true' not in package:ui.adb('shell','pm','grant',PKG,'android.permission.'+permission,timeout=50)
launch();ui.wait('백그라운드에서도 수신 가능');before=identity();wait_for(foreground,'visible trusted UI starts connectedDevice FGS');screen('foreground-send')
ui.adb('shell','input','keyevent','HOME');wait_for(foreground,'Home retains real FGS');ui.adb('shell','input','keyevent','SLEEP');wait_for(foreground,'screen off retains real FGS')
# Task Manager stop is an OS operation, not the app's own cleanup action. No sticky/boot restart is scheduled.
ui.adb('shell','cmd','activity','stop-app',PKG);wait_for(lambda:not foreground(),'OS Task Manager stop removes FGS');time.sleep(3);assert not foreground()
launch();ui.wait('백그라운드에서도 수신 가능');wait_for(foreground,'explicit app reopen resumes saved trust after OS stop');assert before==identity()
ui.adb('shell','am','force-stop',PKG);time.sleep(3);wait_for(lambda:not foreground(),'force-stop stays stopped')
launch();ui.wait('백그라운드에서도 수신 가능');assert before==identity()
ui.adb('shell','pm','revoke',PKG,'android.permission.POST_NOTIFICATIONS',timeout=50);launch();ui.wait('백그라운드 수신 중 · 알림 표시가 허용되지 않았습니다');wait_for(foreground,'notification denial permits FGS with explicit UI warning');screen('notification-denied')
ui.adb('shell','pm','grant',PKG,'android.permission.POST_NOTIFICATIONS');ui.wait('백그라운드에서도 수신 가능')
ui.adb('shell','pm','revoke',PKG,'android.permission.ACCESS_LOCAL_NETWORK',timeout=50);launch();ui.wait('로컬 네트워크 권한을 허용하세요');wait_for(lambda:not foreground(),'LAN permission denial opens no foreground listener');screen('lan-permission-denied')
ui.adb('shell','pm','grant',PKG,'android.permission.ACCESS_LOCAL_NETWORK');ui.wait('백그라운드에서도 수신 가능');wait_for(foreground,'visible UI resumes after actual LAN permission grant')
ui.tap('설정');ui.tap('신뢰하는 집 네트워크입니다');ui.wait('설정에서 집 네트워크를 확인하세요');wait_for(lambda:not foreground(),'trust revoked stops transport');screen('untrusted-lan')
ui.tap('신뢰하는 집 네트워크입니다');ui.wait('백그라운드에서도 수신 가능');wait_for(foreground,'explicit trust approval restarts');ui.tap('보내기')
# Cold start with receiving off must retain real history instead of an empty Service snapshot.
ui.tap('설정');ui.tap('송수신 사용');ui.wait('송수신 중지됨');ui.adb('shell','am','force-stop',PKG);launch();ui.wait('송수신 중지됨');ui.tap('기록');ui.wait('mac-background.bin');time.sleep(2);ui.wait('mac-background.bin');screen('history-receiving-off');checks.append('cold disabled receiver retains actual history');ui.tap('설정');ui.tap('송수신 사용');ui.wait('백그라운드에서도 수신 가능');ui.tap('보내기')
# An expired reconnect grace never becomes a background start exemption.
ui.adb('shell','input','keyevent','HOME');ui.adb('shell','svc','wifi','disable')
wait_for(lambda:not foreground(),'LAN loss expires bounded existing-FGS grace',timeout=75)
ui.adb('shell','svc','wifi','enable');time.sleep(3);assert not foreground();checks.append('no automatic FGS resurrection after expired grace')
launch();ui.wait('백그라운드에서도 수신 가능',timeout=45);wait_for(foreground,'normal visible reopen resumes previous trusted LAN');assert before==identity();screen('restored-send')
ui.adb('shell','cmd','statusbar','expand-notifications');ui.tree();screen('ongoing-notification');ui.adb('shell','cmd','statusbar','collapse')
(OUT/'verification.json').write_text(json.dumps({'checks':checks,'passed':len(checks),'identityPreserved':True,'physicalDevice':False},ensure_ascii=False,indent=2)+'\n');print('Background policy passed:',len(checks),flush=True)
