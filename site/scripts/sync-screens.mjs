// Screenshots live once in site/screens/; each site gets a gitignored copy in public/screens/.
// www also gets resized WebP variants (720w, 1440w) so pages never ship the 2880px PNGs.
import { cpSync, mkdirSync, existsSync, rmSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const site = process.argv[2];
const target = join(root, site, "public", "screens");
const source = join(root, "screens");
rmSync(target, { recursive: true, force: true });
mkdirSync(target, { recursive: true });
if (!existsSync(source)) process.exit(0);

if (site === "www") {
  const sharp = createRequire(join(root, site, "package.json"))("sharp");
  for (const f of readdirSync(source).filter((f) => f.endsWith(".png"))) {
    const base = f.slice(0, -4);
    for (const w of [720, 1440]) {
      await sharp(join(source, f)).resize({ width: w }).webp({ quality: 82 }).toFile(join(target, `${base}-${w}.webp`));
    }
  }
} else {
  cpSync(source, target, { recursive: true });
}
