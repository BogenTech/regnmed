// Screenshots live once in site/screens/; each site gets a gitignored copy in public/screens/.
import { cpSync, mkdirSync, existsSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const target = join(root, process.argv[2], "public", "screens");
rmSync(target, { recursive: true, force: true });
mkdirSync(target, { recursive: true });
if (existsSync(join(root, "screens"))) cpSync(join(root, "screens"), target, { recursive: true });
