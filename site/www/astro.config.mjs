import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";

export default defineConfig({
  site: "https://www.regnmed.no",
  integrations: [sitemap()],
  build: { inlineStylesheets: "auto" },
});
