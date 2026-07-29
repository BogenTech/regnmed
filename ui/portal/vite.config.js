import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

// Appen serveres av regnmed-api på roten (#76 er flippet: Svelte-appen
// ER portalen). Ingen SSR — index.html er statisk, rutingen skjer i
// hashen, og API-et bor på samme origin.
export default defineConfig({
  base: "/",
  plugins: [svelte(), tailwindcss()],
  build: {
    // dist sjekkes inn (app.css-presedensen): Rust-bygget leser den med
    // include_dir og trenger aldri Node.
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    // Dev-serveren proxyer API-et til en kjørende regnmed-api, så
    // hot-reload virker mot ekte data uten CORS.
    proxy: {
      "/companies": "http://localhost:8080",
      "/me": "http://localhost:8080",
      "/firms": "http://localhost:8080",
      "/registry": "http://localhost:8080",
      "/auth": "http://localhost:8080",
      "/portal-config": "http://localhost:8080",
      "/anchors": "http://localhost:8080",
    },
  },
});
