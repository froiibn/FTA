import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, join } from "node:path";

// Give both Cargo and the Tauri subprocess the same discovered toolchain, including fresh rustup installs.
export function rustEnvironment() {
  const extension = process.platform === "win32" ? ".exe" : "";
  const cargoBin = join(process.env.CARGO_HOME ?? join(homedir(), ".cargo"), "bin");
  const env = { ...process.env, PATH: `${cargoBin}${delimiter}${process.env.PATH ?? ""}` };
  if (process.platform === "darwin" && !env.MACOSX_DEPLOYMENT_TARGET) {
    // Match sidecar and app deployment targets; separate compiler defaults can silently produce mixed bundles.
    const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
    const minimum = config.bundle?.macOS?.minimumSystemVersion;
    if (typeof minimum === "string") env.MACOSX_DEPLOYMENT_TARGET = minimum;
  }
  const resolve = (name, override) => override ?? (existsSync(join(cargoBin, `${name}${extension}`)) ? join(cargoBin, `${name}${extension}`) : name);
  return { env, cargo: resolve("cargo", process.env.CARGO), rustc: resolve("rustc", process.env.RUSTC) };
}
