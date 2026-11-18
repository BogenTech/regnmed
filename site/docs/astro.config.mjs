import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://docs.regnmed.no",
  integrations: [
    starlight({
      title: "regnmed docs",
      description:
        "Brukerveiledning, utviklerdokumentasjon og teknisk dokumentasjon for regnmed, regnskapssystemet du kan etterprøve.",
      defaultLocale: "root",
      locales: { root: { label: "Norsk", lang: "nb" } },
      logo: { src: "./src/assets/logo.svg", alt: "regnmed" },
      favicon: "/favicon.svg",
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/BogenTech/regnmed" }],
      editLink: { baseUrl: "https://github.com/BogenTech/regnmed/edit/main/site/docs/" },
      customCss: ["./src/styles/docs.css"],
      lastUpdated: false,
      sidebar: [
        { label: "Brukerveiledning", items: [{ autogenerate: { directory: "brukerveiledning" } }] },
        { label: "For utviklere", items: [{ autogenerate: { directory: "utviklere" } }] },
        { label: "Teknisk dokumentasjon", collapsed: true, items: [{ autogenerate: { directory: "teknisk" } }] },
      ],
    }),
  ],
});
