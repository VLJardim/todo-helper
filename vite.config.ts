import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";

// Tauri loads devUrl http://localhost:1420, so the port is fixed. Cargo watches src-tauri, so Vite doesn't.
export default defineConfig({
  plugins: [tailwindcss()],
  clearScreen: false, // keep Rust errors on screen
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
});
