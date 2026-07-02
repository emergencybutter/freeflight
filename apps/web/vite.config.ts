import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// sql.js ships CJS/UMD builds with no real ESM `default` export; Vite's
// esbuild dependency pre-bundling is what synthesizes that interop, so
// sql.js must NOT be excluded from optimizeDeps (the opposite of the usual
// advice for wasm-heavy packages) or the dev server 500s on import.
export default defineConfig({
  plugins: [react()],
});
