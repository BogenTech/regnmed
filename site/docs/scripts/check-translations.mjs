// Fails when an English developer page no longer mirrors its Norwegian page structurally:
// same files, same headings, code fences, fence lengths and endpoint tokens. Prose is not compared.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = new URL("../src/content/docs/", import.meta.url).pathname;
const list = (d) => readdirSync(join(root, d), { recursive: true }).filter((f) => f.endsWith(".md")).sort();
const shape = (t) => ({
  headings: (t.match(/^#{2,4} /gm) || []).length,
  fences: (t.match(/```[^\n]*\n[\s\S]*?```/g) || []).map((f) => f.split("\n").length),
  endpoints: [...new Set((t.match(/`(?:GET|POST|PUT|DELETE|PATCH) \/[^`]*`/g) || []).map((e) => e.replace(/<[^>]*>/g, "<>")))].sort(),
});
const nb = list("developers"), en = list("en/developers");
const problems = [];
for (const f of new Set([...nb, ...en])) {
  if (!nb.includes(f) || !en.includes(f)) { problems.push(`${f}: exists in only one locale`); continue; }
  const a = shape(readFileSync(join(root, "developers", f), "utf8")), b = shape(readFileSync(join(root, "en/developers", f), "utf8"));
  if (a.headings !== b.headings) problems.push(`${f}: headings ${a.headings} vs ${b.headings}`);
  if (JSON.stringify(a.fences) !== JSON.stringify(b.fences)) problems.push(`${f}: code fences differ`);
  if (JSON.stringify(a.endpoints) !== JSON.stringify(b.endpoints)) problems.push(`${f}: endpoints differ`);
}
if (problems.length) { console.error("English developer docs are out of step:\n  " + problems.join("\n  ")); process.exit(1); }
console.log(`translations: ${nb.length} pages in step`);
