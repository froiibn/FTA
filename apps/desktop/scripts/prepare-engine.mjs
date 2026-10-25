import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { rustEnvironment } from "./toolchain.mjs";

// Build the real engine before bundling; a desktop installer must never ship a fake sidecar.
const workspace = fileURLToPath(new URL("../../../", import.meta.url));
const desktop = fileURLToPath(new URL("../", import.meta.url));
const extension = process.platform === "win32" ? ".exe" : "";
const { env, rustc, cargo } = rustEnvironment();

// Capture only compiler metadata; build output remains visible and no shell is used.
const version = spawnSync(rustc, ["-vV"], { cwd: workspace, encoding: "utf8", env });
if (version.error || version.status !== 0) throw new Error(`Rust toolchain is unavailable: ${version.error?.message ?? version.stderr}`);
const host = version.stdout.match(/^host: (.+)$/m)?.[1];
if (!host) throw new Error("rustc did not report a host target");
const sidecars = ["fta-engine", "fta-mcp"];
// The fixed Windows helper is a separate UAC target; never elevate the whole desktop/engine.
if (process.platform === "win32") sidecars.push("fta-network-helper");
const build = spawnSync(cargo, ["build", "--locked", "--release", ...sidecars.flatMap((name) => ["-p", name])], { cwd: workspace, stdio: "inherit", env });
if (build.error || build.status !== 0) throw new Error(`Engine build failed: ${build.error?.message ?? build.status}`);
// Cargo metadata resolves environment and .cargo/config target directories relative to Cargo's own rules.
const metadata = spawnSync(cargo, ["metadata", "--no-deps", "--format-version", "1"], { cwd: workspace, encoding: "utf8", env });
if (metadata.error || metadata.status !== 0) throw new Error(`Cargo metadata failed: ${metadata.error?.message ?? metadata.stderr}`);
const targetRoot = JSON.parse(metadata.stdout).target_directory;
if (typeof targetRoot !== "string") throw new Error("Cargo did not report its target directory");
for (const name of sidecars) {
  const source = join(targetRoot, "release", `${name}${extension}`);
  const destination = join(desktop, "src-tauri", "binaries", `${name}-${host}${extension}`);
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(source, destination);
}
console.log(`Prepared ${sidecars.join(", ")} sidecars for ${host}`);
