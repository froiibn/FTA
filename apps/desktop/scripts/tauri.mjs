import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { rustEnvironment } from "./toolchain.mjs";

// Keep the Tauri CLI and sidecar preparation in one environment; setting PATH in a child cannot fix its parent.
const desktop = fileURLToPath(new URL("../", import.meta.url));
const { env } = rustEnvironment();
const bundleEngine = process.argv.includes("--bundle-engine");
const args = process.argv.slice(2).filter((argument) => argument !== "--bundle-engine");
if (bundleEngine) {
  if (args.some((argument) => argument === "--target" || argument.startsWith("--target="))) {
    throw new Error("The bundle command currently builds the native host target; cross-target packaging is not supported yet.");
  }
  const prepared = spawnSync(process.execPath, [fileURLToPath(new URL("./prepare-engine.mjs", import.meta.url))], { cwd: desktop, env, stdio: "inherit" });
  if (prepared.error || prepared.status !== 0) throw new Error(`Engine preparation failed: ${prepared.error?.message ?? prepared.status}`);
  args.push("--config", "src-tauri/tauri.bundle.conf.json");
}
const result = spawnSync(process.execPath, [fileURLToPath(new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url)), ...args], { cwd: desktop, env, stdio: "inherit" });
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
