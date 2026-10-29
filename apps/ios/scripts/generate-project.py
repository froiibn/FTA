#!/usr/bin/env python3
"""Generate the checked-in Xcode project without a third-party generator or machine-specific paths."""
from pathlib import Path
import hashlib
import json
import subprocess
root = Path(__file__).resolve().parents[1]
# Preserve Xcode edits when regenerating; team credentials belong in ignored LocalSigning.xcconfig.
existing_project = root/'FTA.xcodeproj/project.pbxproj'
previous = json.loads(subprocess.check_output(['plutil','-convert','json','-o','-',str(existing_project)])) if existing_project.exists() else {}
objects = {}
def ident(name): return hashlib.sha256(name.encode()).hexdigest()[:24].upper()
def obj(name, text): objects[ident(name)] = text; return ident(name)
def q(s): return json.dumps(s)
def build_value(value):
 if isinstance(value,list): return '(' + ','.join(build_value(item) for item in value) + ')'
 if isinstance(value,dict): return '{' + ''.join(f'{q(key)} = {build_value(item)};' for key,item in value.items()) + '}'
 return q(str(value))
def array(items): return '(' + ','.join(items) + ')'
files=[]; builds=[]
for file in sorted((root/'FTA').glob('*.swift')):
 ref=obj(str(file.name),f'isa = PBXFileReference; lastKnownFileType = sourcecode.swift; path = {q("FTA/"+file.name)}; sourceTree = "<group>";')
 files.append(ref); builds.append(obj('build'+file.name,f'isa = PBXBuildFile; fileRef = {ref};'))
signing=obj('Signing.xcconfig','isa = PBXFileReference; lastKnownFileType = text.xcconfig; path = FTA/Signing.xcconfig; sourceTree = "<group>";')
files.append(signing)
asset=obj('assets','isa = PBXFileReference; lastKnownFileType = folder.assetcatalog; path = FTA/Assets.xcassets; sourceTree = "<group>";')
files.append(asset)
assetbuild=obj('assetbuild',f'isa = PBXBuildFile; fileRef = {asset};')
resources=obj('resources',f'isa = PBXResourcesBuildPhase; buildActionMask = 2147483647; files = ({assetbuild}); runOnlyForDeploymentPostprocessing = 0;')
product=obj('app','isa = PBXFileReference; explicitFileType = wrapper.application; path = FTA.app; sourceTree = BUILT_PRODUCTS_DIR;')
products=obj('products',f'isa = PBXGroup; children = ({product}); name = Products; sourceTree = "<group>";')
group=obj('group',f'isa = PBXGroup; children = {array(files+[products])}; sourceTree = "<group>";')
sources=obj('sources',f'isa = PBXSourcesBuildPhase; buildActionMask = 2147483647; files = {array(builds)}; runOnlyForDeploymentPostprocessing = 0;')
frameworks=obj('frameworks','isa = PBXFrameworksBuildPhase; buildActionMask = 2147483647; files = (); runOnlyForDeploymentPostprocessing = 0;')
rust=obj('rust',f'isa = PBXShellScriptBuildPhase; alwaysOutOfDate = 1; buildActionMask = 2147483647; files = (); inputPaths = (); outputPaths = ("$(DERIVED_FILE_DIR)/libfta_ios.a"); runOnlyForDeploymentPostprocessing = 0; shellPath = /bin/sh; shellScript = {q(chr(34)+"$SRCROOT/scripts/build-rust.sh"+chr(34))}; name = "Build Rust engine";')
base={'ARCHS':'arm64','IPHONEOS_DEPLOYMENT_TARGET':'17.0','SDKROOT':'iphoneos','SWIFT_VERSION':'5.0','SWIFT_OBJC_BRIDGING_HEADER':'FTA/FTABridge.h','CLANG_ENABLE_MODULES':'YES','ENABLE_USER_SCRIPT_SANDBOXING':'NO','SUPPORTED_PLATFORMS':'iphoneos iphonesimulator','TARGETED_DEVICE_FAMILY':'1,2','CODE_SIGN_STYLE':'Automatic','CODE_SIGN_ENTITLEMENTS':'FTA/FTA.entitlements','PRODUCT_BUNDLE_IDENTIFIER':'app.fta.ios','PRODUCT_NAME':'FTA','INFOPLIST_FILE':'FTA/Info.plist','GENERATE_INFOPLIST_FILE':'NO','LIBRARY_SEARCH_PATHS':'$(inherited) $(DERIVED_FILE_DIR)','OTHER_LDFLAGS':'$(inherited) -lfta_ios -lsqlite3 -liconv -framework Security -framework SystemConfiguration -framework CoreFoundation','SWIFT_EMIT_LOC_STRINGS':'YES','ENABLE_PREVIEWS':'YES','ASSETCATALOG_COMPILER_APPICON_NAME':'AppIcon'}
def configs(name,base):
 refs=[]
 for mode in ['Debug','Release']:
  settings=dict(base); settings.update(previous.get('objects',{}).get(ident(name+mode),{}).get('buildSettings',{})); settings['SWIFT_OPTIMIZATION_LEVEL']='-Onone' if mode=='Debug' else '-O'; settings['DEBUG_INFORMATION_FORMAT']='dwarf'
  refs.append(obj(name+mode,f'isa = XCBuildConfiguration; baseConfigurationReference = {signing}; buildSettings = {{'+''.join(f'{q(k)} = {build_value(v)};' for k,v in settings.items())+'}; name = '+mode+';'))
 return obj(name+'configs',f'isa = XCConfigurationList; buildConfigurations = {array(refs)}; defaultConfigurationIsVisible = 0; defaultConfigurationName = Release;')
target=obj('target',f'isa = PBXNativeTarget; buildConfigurationList = {configs("app",base)}; buildPhases = {array([rust,sources,frameworks,resources])}; buildRules = (); dependencies = (); name = FTA; productName = FTA; productReference = {product}; productType = "com.apple.product-type.application";')
# UI tests run only through the local Xcode simulator and exercise real native controls.
testfile=obj('testfile','isa = PBXFileReference; lastKnownFileType = sourcecode.swift; path = Tests/FTAFlowTests.swift; sourceTree = "<group>";')
testbuild=obj('testbuild',f'isa = PBXBuildFile; fileRef = {testfile};')
testproduct=obj('testproduct','isa = PBXFileReference; explicitFileType = wrapper.cfbundle; path = FTAFlowTests.xctest; sourceTree = BUILT_PRODUCTS_DIR;')
objects[products]=objects[products].replace(f'({product})',f'({product},{testproduct})')
objects[group]=objects[group].replace(array(files+[products]),array(files+[testfile,products]))
testsource=obj('testsource',f'isa = PBXSourcesBuildPhase; buildActionMask = 2147483647; files = ({testbuild}); runOnlyForDeploymentPostprocessing = 0;')
proxy=obj('proxy',f'isa = PBXContainerItemProxy; containerPortal = {ident("project")}; proxyType = 1; remoteGlobalIDString = {target}; remoteInfo = FTA;')
dependency=obj('dependency',f'isa = PBXTargetDependency; target = {target}; targetProxy = {proxy};')
testbase={k:v for k,v in base.items() if k not in ['LIBRARY_SEARCH_PATHS','OTHER_LDFLAGS','INFOPLIST_FILE','PRODUCT_NAME','PRODUCT_BUNDLE_IDENTIFIER','GENERATE_INFOPLIST_FILE']}
testbase.update({'PRODUCT_NAME':'FTAFlowTests','PRODUCT_BUNDLE_IDENTIFIER':'app.fta.ios.flowtests','GENERATE_INFOPLIST_FILE':'YES','TEST_TARGET_NAME':'FTA'})
testtarget=obj('testtarget',f'isa = PBXNativeTarget; buildConfigurationList = {configs("tests",testbase)}; buildPhases = ({testsource}); buildRules = (); dependencies = ({dependency}); name = FTAFlowTests; productName = FTAFlowTests; productReference = {testproduct}; productType = "com.apple.product-type.bundle.ui-testing";')
project=obj('project',f'isa = PBXProject; attributes = {{LastUpgradeCheck = 2700; }}; buildConfigurationList = {configs("project",{})}; compatibilityVersion = "Xcode 14.0"; developmentRegion = ko; hasScannedForEncodings = 0; knownRegions = (ko,en,Base); mainGroup = {group}; productRefGroup = {products}; projectDirPath = ""; projectRoot = ""; targets = ({target},{testtarget});')
out=root/'FTA.xcodeproj';out.mkdir(exist_ok=True)
(out/'project.pbxproj').write_text('// !$*UTF8*$!\n{ archiveVersion = 1; classes = {}; objectVersion = 56; objects = {\n'+''.join(f'{key} = {{ {value} }};\n' for key,value in objects.items())+f'}}; rootObject = {project}; }}\n')
schemes=out/'xcshareddata/xcschemes';schemes.mkdir(parents=True,exist_ok=True)
ref=f'<BuildableReference BuildableIdentifier="primary" BlueprintIdentifier="{target}" BuildableName="FTA.app" BlueprintName="FTA" ReferencedContainer="container:FTA.xcodeproj"/>'
(schemes/'FTA.xcscheme').write_text(f'''<?xml version="1.0" encoding="UTF-8"?><Scheme LastUpgradeVersion="2700" version="1.3"><BuildAction parallelizeBuildables="YES" buildImplicitDependencies="YES"><BuildActionEntries><BuildActionEntry buildForTesting="YES" buildForRunning="YES" buildForProfiling="YES" buildForArchiving="YES" buildForAnalyzing="YES">{ref}</BuildActionEntry></BuildActionEntries></BuildAction><TestAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" shouldUseLaunchSchemeArgsEnv="YES"><Testables><TestableReference skipped="NO"><BuildableReference BuildableIdentifier="primary" BlueprintIdentifier="{testtarget}" BuildableName="FTAFlowTests.xctest" BlueprintName="FTAFlowTests" ReferencedContainer="container:FTA.xcodeproj"/></TestableReference></Testables></TestAction><LaunchAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" launchStyle="0" useCustomWorkingDirectory="NO" ignoresPersistentStateOnLaunch="NO" debugDocumentVersioning="YES" debugServiceExtension="internal" allowLocationSimulation="YES"><BuildableProductRunnable runnableDebuggingMode="0">{ref}</BuildableProductRunnable></LaunchAction><ProfileAction buildConfiguration="Release" shouldUseLaunchSchemeArgsEnv="YES" savedToolIdentifier="" useCustomWorkingDirectory="NO" debugDocumentVersioning="YES"><BuildableProductRunnable runnableDebuggingMode="0">{ref}</BuildableProductRunnable></ProfileAction><AnalyzeAction buildConfiguration="Debug"/><ArchiveAction buildConfiguration="Release" revealArchiveInOrganizer="YES"/></Scheme>''')
