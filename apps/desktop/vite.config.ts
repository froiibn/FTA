import { defineConfig } from "vite";

// Match the local-only Tauri dev URL; a fixed named default avoids accidental LAN exposure.
const DEFAULT_DEV_PORT = 1420;
export default defineConfig({
  server: { port: DEFAULT_DEV_PORT, strictPort: true, host: "127.0.0.1" },
  clearScreen: false,
});
