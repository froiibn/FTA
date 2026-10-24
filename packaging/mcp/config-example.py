#!/usr/bin/env python3
"""Print client configuration from physical paths without modifying any user configuration."""

import argparse
import json
import os
from pathlib import Path

DEFAULT_TOOL_TIMEOUT_SECONDS = 3600


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", choices=["codex", "claude"], required=True)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--allow-dir", action="append", type=Path, default=[])
    for name in ("config-dir", "state-dir", "runtime-dir"):
        parser.add_argument(f"--{name}", type=Path)
    args = parser.parse_args()

    # Resolve from this checked-out project, never from a guessed user's home directory.
    project = Path(__file__).resolve().parents[2]
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    if not target.is_absolute():
        target = project / target
    default_binary = target / "release" / ("fta-mcp.exe" if os.name == "nt" else "fta-mcp")
    binary = (args.binary or default_binary).resolve(strict=True)
    if not binary.is_file():
        parser.error("--binary must name a compiled regular executable")
    server_args = []
    for root in args.allow_dir:
        root = root.resolve(strict=True)
        if not root.is_dir():
            parser.error("--allow-dir must name an existing directory")
        server_args.extend(["--allow-dir", str(root)])
    for option in ("config_dir", "state_dir", "runtime_dir"):
        value = getattr(args, option)
        if value is not None:
            server_args.extend(["--" + option.replace("_", "-"), str(value.resolve())])

    if args.client == "claude":
        print(json.dumps({"mcpServers": {"fta": {"type": "stdio", "command": str(binary), "args": server_args}}}, ensure_ascii=False, indent=2))
    else:
        print("[mcp_servers.fta]")
        print("command = " + json.dumps(str(binary), ensure_ascii=False))
        print("args = " + json.dumps(server_args, ensure_ascii=False))
        # This is a client-side example policy, not a timeout on the FTA transfer engine.
        print(f"tool_timeout_sec = {DEFAULT_TOOL_TIMEOUT_SECONDS}")


if __name__ == "__main__":
    main()
