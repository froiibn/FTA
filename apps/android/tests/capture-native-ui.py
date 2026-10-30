#!/usr/bin/env python3
"""Capture verified native destinations, avoiding stale pre-navigation screenshot frames."""
import importlib.util
from pathlib import Path

spec=importlib.util.spec_from_file_location('ui',Path(__file__).with_name('ui-smoke.py'))
ui=importlib.util.module_from_spec(spec)
spec.loader.exec_module(ui)
assert ui.adb('shell','getprop','ro.kernel.qemu').strip()==b'1','Dedicated emulator only'
ui.launch()
try:
 for mode,folder in [('no','light'),('yes','dark')]:
  ui.adb('shell','cmd','uimode','night',mode)
  for tab,name,marker in [('보내기','send','File Transfer Anything'),('기록','history','전송 기록'),('설정','settings','신뢰하는 집 네트워크입니다')]:
   ui.tap(tab);ui.wait(marker)
   # Accessibility idle and the selected native control prove the captured destination, not the previous frame.
   nodes=list(ui.tree().iter('node'))
   assert any(n.get('class')=='android.widget.RadioButton' and n.get('text')==tab and n.get('checked')=='true' for n in nodes)
   target=ui.ROOT/'output/android-native-ui-v2'/folder/(name+'.png')
   target.parent.mkdir(parents=True,exist_ok=True)
   target.write_bytes(ui.adb('exec-out','screencap','-p'))
   print(folder,name,'verified destination',flush=True)
finally:
 # Leave the dedicated emulator in the documented light starting appearance.
 ui.adb('shell','cmd','uimode','night','no')
 ui.tap('보내기');ui.wait('File Transfer Anything')
