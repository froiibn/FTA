#!/usr/bin/env python3
"""Extract runtime attribution from the installed, exact Rust toolchain; no network/source installs."""
from pathlib import Path
from html import unescape
from html.parser import HTMLParser
import hashlib
import json
import re
import subprocess

IOS = Path(__file__).resolve().parents[1]
TARGET = "aarch64-apple-ios"

class Rules(HTMLParser):
    # Preserve the provider's nested path/copyright exceptions, excluding compiler-only GPL trees.
    def __init__(self):
        super().__init__(); self.stack=[]; self.nodes=[]
    def handle_starttag(self,tag,attrs):
        if tag == "div":
            node={"parent":self.stack[-1] if self.stack else None,"text":[]}
            self.nodes.append(node); self.stack.append(node)
    def handle_endtag(self,tag):
        if tag == "div" and self.stack: self.stack.pop()
    def handle_data(self,data):
        if self.stack: self.stack[-1]["text"].append(data)

def rust(*args):
    return subprocess.check_output(["rustc",*args],text=True).strip()

def main():
    version=dict(line.split(": ",1) for line in rust("--version","--verbose").splitlines() if ": " in line)
    root=Path(rust("--print","sysroot")); doc=root/"share/doc/rust/COPYRIGHT.html"
    source=doc.read_text(); section='<h2 id="out-of-tree-dependencies"'
    before,after=source.split(section,1)
    target=Path(rust("--print","target-libdir","--target",TARGET))
    families={re.match(r"lib(.+?)-",p.name).group(1) for p in target.glob("*.rlib")}
    assert families and "std" in families
    parser=Rules();parser.feed(before)
    in_tree=[]
    for node in parser.nodes:
        text=" ".join(" ".join(node["text"]).split())
        match=re.search(r"File/Directory: (.*?)(?= License:| Copyright:| Exceptions:|$)",text)
        if not match: continue
        parent=node["parent"]; parent_path=parent.get("path","") if parent else ""
        path=str(Path(parent_path)/match.group(1)) if parent_path not in ("", ".") else match.group(1)
        node["path"]=path
        if path == "." or path.startswith("library/") or path == "src/llvm-project": in_tree.append(text.replace("File/Directory: "+match.group(1),"File/Directory: "+path,1))
    packages=[]; blocks=[]
    for block in re.split(r"(?=<h3>📦 )",after):
        heading=re.match(r"<h3>📦 (.*)-(\d[^<]*)</h3>",block)
        if not heading or heading.group(1).replace("-","_") not in families: continue
        # The official inventory may list several versions/build variants in one family. Include them
        # explicitly rather than pretending a Cargo application graph inventories sysroot libraries.
        fields=[]
        for field in ["URL","In libstd","Authors","License"]:
            match=re.search(r"<p><b>"+field+r":</b>(.*?)</p>",block,re.S)
            if match: fields.append(field+": "+unescape(re.sub(r"<[^>]+>","",match.group(1))).strip())
        notices=[unescape(value) for value in re.findall(r"<pre>(.*?)</pre>",block,re.S)]
        if not notices: raise SystemExit("Missing toolchain notice: "+heading.group(0))
        name=heading.group(1)+" "+heading.group(2);packages.append(name)
        blocks.append(name+"\n"+"\n".join(fields)+"\n"+"\n\n".join(notices))
    license_names=["MIT.txt","Apache-2.0.txt","Unicode-3.0.txt","BSD-2-Clause.txt","LLVM-exception.txt","NCSA.txt"]
    full_licenses=[name+"\n"+(root/"share/doc/rust/licenses"/name).read_text() for name in license_names]
    text=("Rust "+version["release"]+" standard libraries and runtime notices\n"
          "Source: the installed toolchain's official COPYRIGHT.html and license files.\n"
          "This includes dependency families present in the iOS target sysroot, including build/debug\n"
          "and platform variants. Inclusion does not assert every listed version is linked into FTA.\n"
          "No implementation source is included. Original license/notice text is preserved.\n\n"
          "IN-TREE RUNTIME COPYRIGHTS\n\n"+"\n\n".join(in_tree)+"\n\n"
          "RUNTIME DEPENDENCY FAMILY NOTICES\n\n"+"\n\n".join(blocks)+"\n\n"
          "RUST / UNICODE / COMPILER-RUNTIME LICENSES\n\n"+"\n\n".join(full_licenses)+"\n")
    destination=IOS/"LegalSources"/("rust-runtime-"+version["release"]);destination.mkdir(exist_ok=True)
    filename="RustRuntimeNotices.txt";(destination/filename).write_text(text)
    manifest=(root/"lib/rustlib/multirust-channel-manifest.toml").read_text()
    package=manifest.split("[pkg.rust-docs.target."+version["host"]+"]",1)[1].split("[",1)[0]
    distribution=re.search(r'^xz_url = "([^"]+)"',package,re.M).group(1)
    provenance={"release":version["release"],"commit":version["commit-hash"],"target":TARGET,
                "source_distribution":distribution,"source_document_sha256":hashlib.sha256(doc.read_bytes()).hexdigest(),
                "target_library_families":sorted(families),"dependency_family_notices":packages,
                "files":{filename:hashlib.sha256(text.encode()).hexdigest()}}
    (destination/"provenance.json").write_text(json.dumps(provenance,ensure_ascii=False,indent=2)+"\n")
    print("Extracted Rust runtime notices:",len(packages),"dependency variants;",len(text.encode()),"bytes")

if __name__ == "__main__": main()
