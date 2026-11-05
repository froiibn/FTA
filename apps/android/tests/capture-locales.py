#!/usr/bin/env python3
"""Capture actual localized SDK UI on dedicated emulators, without adding mock peers or transfer records."""
import argparse,importlib.util,json,os,re,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3];parser=argparse.ArgumentParser();parser.add_argument('--serial',required=True);parser.add_argument('--family',required=True);parser.add_argument('--locales',default='en,ko,ja,zh-Hans,zh-Hant,de,fr,es,pt-BR,it');parser.add_argument('--font-scale',default='1.0');parser.add_argument('--night',choices=['yes','no'],default='no');parser.add_argument('--scenes',default='send,photos,history,settings');args=parser.parse_args();os.environ['ANDROID_SERIAL']=args.serial
spec=importlib.util.spec_from_file_location('ui',Path(__file__).with_name('ui-smoke.py'));ui=importlib.util.module_from_spec(spec);spec.loader.exec_module(ui)
if ui.adb('shell','getprop','ro.kernel.qemu').strip()!=b'1':raise SystemExit('Only dedicated emulator capture is allowed; never change a user phone language/settings')
out=ROOT/'output/android-play-v7/raw'/args.family;out.mkdir(parents=True,exist_ok=True);originalFont=ui.adb('shell','settings','get','system','font_scale').decode().strip();report=[]
def nodes():return [n for n in ui.tree().iter('node') if n.get('package')=='app.fta.android']
# Keep locale capture compatible with accessible icon actions and native text menu choices.
def text_node(tree,text,cls=None):return next((n for n in tree if (n.get('text')==text or n.get('content-desc')==text) and (cls is None or n.get('class')==cls)),None)
def capture(locale,scene):
 tree=nodes();assert tree,'FTA not foreground';assert locale=='ko' or not any(re.search('[가-힣]',n.get('text','')) for n in tree),'Korean string leakage';path=out/f'{locale}-{scene}.png';path.write_bytes(ui.adb('exec-out','screencap','-p'));return tree
try:
 ui.adb('shell','settings','put','system','font_scale',args.font_scale);ui.adb('shell','cmd','uimode','night',args.night)
 for locale in args.locales.split(','):
  expected='en' if locale=='nl-NL' else locale;catalog=json.loads((ROOT/f'apps/android/Localization/{expected}.json').read_text());ui.adb('shell','cmd','locale','set-app-locales','app.fta.android','--user','0','--locales',locale);ui.adb('shell','am','start','-W','-n','app.fta.android/.MainActivity')
  tree=nodes();nav=text_node(tree,catalog['send'],'android.widget.RadioButton');assert nav is not None,(locale,'send navigation');ui.tap_node(nav);capture(locale,'send')
  tree=nodes();photo=text_node(tree,catalog['photos'],'android.widget.ImageButton');assert photo is not None,(locale,'photo button');ui.tap_node(photo);menu=capture(locale,'photos');assert text_node(menu,catalog['choose_from_photos']) is not None,(locale,'gallery choice');assert text_node(menu,catalog['take_photo']) is not None,(locale,'camera choice');ui.adb('shell','input','keyevent','KEYCODE_BACK')
  for scene,key in [('history','history'),('settings','settings')]:
   if scene not in args.scenes.split(','):continue
   tree=nodes();nav=text_node(tree,catalog[key],'android.widget.RadioButton');assert nav is not None,(locale,key);ui.tap_node(nav);capture(locale,scene)
  report.append({'locale':locale,'scenes':len(args.scenes.split(',')),'foreignKoreanLeakage':False});print(args.family,locale,str(len(args.scenes.split(',')))+' actual screens captured',flush=True)
finally:
 ui.adb('shell','settings','put','system','font_scale',originalFont if originalFont!='null' else '1.0')
(out/'capture-report.json').write_text(json.dumps({'serial':args.serial,'fontScale':args.font_scale,'night':args.night,'locales':report},indent=2))
