// The build is committed and embedded in the `submilli` binary, which serves it from
// the playground's control listener. CI rebuilds it and fails on any difference, so
// the output must depend only on the sources and the lockfile.
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  // The page is served from the listener's root, and its assets from `/assets/`.
  base: "/",
  build: {
    outDir: "../src/commands/playground/web/dist",
    emptyOutDir: true,
    sourcemap: false,
    // Every asset is a file of its own, never inlined into the CSS or script as a
    // data URL, so the content security policy needs no `data:` for fonts.
    assetsInlineLimit: 0,
    // No `<link rel="modulepreload">` polyfill script: one entry chunk needs none.
    modulePreload: { polyfill: false },
  },
});
