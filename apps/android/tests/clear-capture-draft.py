#!/usr/bin/env python3
"""Remove only the known multiline UI-test probe from dedicated emulator screenshots; never edit captured pixels."""
import argparse,importlib.util,os
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--serial',required=True);args=p.parse_args();os.environ['ANDROID_SERIAL']=args.serial
spec=importlib.util.spec_from_file_location('ui',Path(__file__).with_name('ui-smoke.py'));ui=importlib.util.module_from_spec(spec);spec.loader.exec_module(ui)
if ui.adb('shell','getprop','ro.kernel.qemu').strip()!=b'1':raise SystemExit('Dedicated emulator only')
ui.adb('shell','cmd','locale','set-app-locales','app.fta.android','--user','0','--locales','en');ui.adb('shell','am','start','-W','-n','app.fta.android/.MainActivity')
def nodes():return [n for n in ui.tree().iter('node') if n.get('package')=='app.fta.android']
nav=next(n for n in nodes() if n.get('class')=='android.widget.RadioButton' and n.get('text')=='Send');ui.tap_node(nav);editor=next(n for n in nodes() if n.get('class')=='android.widget.EditText');text=editor.get('text','')
if text in ['', 'Text or URL']:print('Draft already empty');raise SystemExit(0)
if text!='one\ntwo\nthree\nfour\nfive\nsix':raise SystemExit('Unexpected draft; refusing to change potentially unrelated data')
ui.tap_node(editor);CTRL_LEFT_KEYCODE=113;A_KEYCODE=29;ui.adb('shell','input','keycombination',str(CTRL_LEFT_KEYCODE),str(A_KEYCODE));ui.adb('shell','input','keyevent','KEYCODE_DEL');ui.adb('shell','input','keyevent','KEYCODE_BACK');empty=next((n for n in nodes() if n.get('text')=='No devices found'),None)
if empty:ui.tap_node(empty)
editor=next(n for n in nodes() if n.get('class')=='android.widget.EditText');assert editor.get('text','') in ['', 'Text or URL'];print('Only the known own UI-test draft cleared through native input events')
