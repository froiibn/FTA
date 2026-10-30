#!/usr/bin/env python3
"""Verify actual native header scrolling on a compact emulator viewport; never invent peers or history."""
import importlib.util,json,re
from pathlib import Path
spec=importlib.util.spec_from_file_location('ui',Path(__file__).with_name('ui-smoke.py'));ui=importlib.util.module_from_spec(spec);spec.loader.exec_module(ui)
assert ui.adb('shell','getprop','ro.kernel.qemu').strip()==b'1','Dedicated emulator only'
out=ui.ROOT/'output/android-scroll-v5';out.mkdir(exist_ok=True)
size=ui.adb('shell','wm','size').decode();override=re.search(r'Override size: (\d+x\d+)',size)
checks=[]
def header(nodes,title):return any(n.get('class')=='android.widget.TextView' and n.get('text')==title for n in nodes)
def nodes():return list(ui.tree().iter('node'))
def image(name):(out/(name+'.png')).write_bytes(ui.adb('exec-out','screencap','-p'))
def tabs(items):return {n.get('text'):n.get('bounds') for n in items if n.get('class')=='android.widget.RadioButton'}
try:
 # A real compact window makes the genuinely short Send page scrollable without fake content or changing the physical phone.
 ui.adb('shell','wm','size','1080x1280');ui.adb('shell','am','force-stop','app.fta.android');ui.launch()
 for tab,title,name,marker in [('보내기','FTA','send','사진'),('기록','전송 기록','history','URL'),('설정','설정','settings','신뢰하는 집 네트워크입니다')]:
  ui.tap(tab);ui.tree();before=nodes();assert header(before,title),(tab,'header initially visible');nav=tabs(before)
  editor=next((n.get('bounds') for n in before if n.get('class')=='android.widget.EditText'),None)
  viewport=next(n for n in before if n.get('class')=='android.widget.ScrollView');box=list(map(int,re.findall(r'\d+',viewport.get('bounds'))));image(name+'-before')
  x=(box[0]+box[2])//2;ui.adb('shell','input','swipe',str(x),str(box[3]-15),str(x),str(box[1]+15),'500');after=nodes()
  if header(after,title):ui.adb('shell','input','swipe',str(x),str(box[3]-15),str(x),str(box[1]+15),'500');after=nodes()
  assert not header(after,title),(tab,'header must leave native visible viewport');assert tabs(after)==nav,(tab,'bottom navigation moved')
  if editor is not None:assert next(n.get('bounds') for n in after if n.get('class')=='android.widget.EditText')==editor,'composer moved'
  image(name+'-after');ui.tree();assert not header(nodes(),title),'service render reset scroll'
  ui.tap(tab);ui.tree();assert not header(nodes(),title),'same-tab reconstruction reset scroll'
  checks.append(tab+': header outside viewport, fixed footer unchanged, service/same-tab update preserves offset');print(checks[-1],flush=True)
 # All three offsets survive leaving and returning to a page.
 for tab,title in [('보내기','FTA'),('기록','전송 기록'),('설정','설정')]:ui.tap(tab);assert not header(nodes(),title),'tab round trip reset '+tab
 checks.append('all tab round trips preserve scroll offsets')
finally:
 ui.adb('shell','wm','size',override.group(1) if override else 'reset')
 ui.adb('shell','am','force-stop','app.fta.android');ui.launch();ui.tap('보내기')
(out/'scroll-verification.json').write_text(json.dumps({'checks':checks,'compactEmulatorWindow':True,'displaySizeRestored':True},ensure_ascii=False,indent=2)+'\n')
