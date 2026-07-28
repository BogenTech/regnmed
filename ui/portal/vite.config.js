import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

// Appen serveres av regnmed-api under /ny (inkrementell migrering, #76).
// Når alle seksjoner har paritet flippes base til /.
export default defineConfig({
  base: "/ny/",
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
