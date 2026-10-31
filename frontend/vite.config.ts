import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

// Vite dev server ini hanya untuk pengembangan UI. Produksi: file dist/
// disajikan oleh server HTTPS desktop (crates/desktop, B1).
export default defineConfig({
  plugins: [preact()],
  server: {
    https: true,
    port: 5173,
    strictPort: false,
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});
