// Pulls the audit-facing docs/ tree into the site as "Teknisk dokumentasjon".
// docs/ stays the single source; this copy is gitignored and rebuilt every build.
import { readFileSync, writeFileSync, readdirSync, rmSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, "../../../docs");
const out = join(here, "../src/content/docs/teknisk");
const GH = "https://github.com/BogenTech/regnmed/blob/main/docs";
// Left out: drafts and operator-internal notes.
const SKIP = new Set(["README.md", "kundevilkar.md", "secrets.md"]);

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const files = readdirSync(src).filter((f) => f.endsWith(".md") && !SKIP.has(f));
const slugs = new Set(files.map((f) => f.slice(0, -3)));

for (const f of files) {
  let text = readFileSync(join(src, f), "utf8");
  const m = text.match(/^# (.+)\n/);
  const title = (m ? m[1] : f.slice(0, -3)).replace(/"/g, "'").replace(/`/g, "");
  if (m) text = text.slice(m[0].length);
  // [x](other.md#a) -> site page when imported, else GitHub; other relative links -> GitHub.
  text = text.replace(/\]\((?!https?:|#|mailto:)([^)\s]+)\)/g, (_, href) => {
    const [path, hash = ""] = href.split("#");
    const slug = path.replace(/\.md$/, "");
    if (path.endsWith(".md") && slugs.has(slug)) return `](/teknisk/${slug}/${hash ? "#" + hash : ""})`;
    return `](${GH}/${path}${hash ? "#" + hash : ""})`;
  });
  // Bare <placeholder> outside code would be parsed as HTML; escape it.
  const parts = text.split(/(```[\s\S]*?```|`[^`\n]*`)/g);
  text = parts.map((p, i) => (i % 2 ? p : p.replace(/<(?!\/?(?:a|br|b|i|em|strong|code|details|summary|sup|sub|table|tr|td|th|thead|tbody|p|ul|li|ol|img|!--)\b)([^>\n]*)>/g, "&lt;$1&gt;"))).join("");
  writeFileSync(join(out, f), `---\ntitle: "${title}"\neditUrl: ${GH.replace("blob", "edit")}/${f}\n---\n\n${text}`);
}
writeFileSync(
  join(out, "index.md"),
  `---\ntitle: Teknisk dokumentasjon\nsidebar:\n  order: 0\n---\n\nDette er regnmeds revisjonsvendte dokumentasjon: hva systemet garanterer, hvor hver garanti håndheves og hvor den testes. Den skrives for revisorer, sertifiseringsprosesser og utviklere, og oppdateres i samme endring som koden. Kilden er [docs/ i repoet](https://github.com/BogenTech/regnmed/tree/main/docs).\n\n${files.map((f) => `- [${f.slice(0, -3)}](/teknisk/${f.slice(0, -3)}/)`).join("\n")}\n`,
);
console.log(`imported ${files.length} technical docs`);
