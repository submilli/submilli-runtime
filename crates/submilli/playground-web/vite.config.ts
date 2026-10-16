// The build is committed and embedded in the `submilli` binary, which serves it from
// the playground's control listener. CI rebuilds it and fails on any difference, so
// the output must depend only on the sources and the lockfile.
import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";

// In development (`npm run dev -- <project folder>`, see `scripts/dev.mjs`) `/api` goes
// to the playground at `PLAYGROUND`. It only answers requests that name its own
// listener, so `Host` and `Origin` are set to it.
export default defineConfig(({ mode }) => {
  const playground = loadEnv(mode, ".", "PLAYGROUND").PLAYGROUND;
  return {
    plugins: [react()],
    server: playground
      ? {
          proxy: {
            "/api": {
              target: playground,
              changeOrigin: true,
              headers: { origin: playground },
              // A request the page gives up (Stop, a closed tab) is given up on the playground
              // too, which stops the chat it started; left alone, the proxy keeps it open.
              configure: (proxy) =>
                proxy.on("proxyReq", (proxyReq, _req, res) => {
                  res.on("close", () => {
                    if (!res.writableFinished) proxyReq.destroy();
                  });
                }),
            },
          },
        }
      : undefined,
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
  };
});
