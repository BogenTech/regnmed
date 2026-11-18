// The sitemap integration lists every route, including the Norwegian fallbacks under /en/.
import { readFileSync, writeFileSync } from "node:fs";
const f = new URL("../dist/sitemap-0.xml", import.meta.url);
const xml = readFileSync(f, "utf8");
const kept = xml
  .replace(/<xhtml:link[^>]*href="https:\/\/docs\.regnmed\.no\/en\/(?:brukerveiledning|teknisk)\/[^"]*"[^>]*\/>/g, "")
  .replace(/<url><loc>https:\/\/docs\.regnmed\.no\/en\/(?:brukerveiledning|teknisk)\/[^<]*<\/loc>.*?<\/url>/g, "");
writeFileSync(f, kept);
console.log(`sitemap: removed ${(xml.match(/<url>/g) || []).length - (kept.match(/<url>/g) || []).length} fallback URLs`);
