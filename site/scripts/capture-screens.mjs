// Captures the portal screenshots for the marketing + docs sites.
// Usage: PPTR=<dir with node_modules/puppeteer-core> node capture-screens.mjs <apiBase> <shotsDir> <outDir> [slug,slug]
// The API must serve a seeded demo (see site/screens/README.md). Auth is faked at the one
// place the portal gets its token: POST /auth/refresh is answered with the seed token.
import { readFileSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";

const [base, dir, out, only] = process.argv.slice(2);
const puppeteer = createRequire(process.env.PPTR + "/")("puppeteer-core");
const token = readFileSync(`${dir}/token.txt`, "utf8").trim();
const company = readFileSync(`${dir}/company.txt`, "utf8").trim();
const firm = process.env.FIRM_ID;
mkdirSync(out, { recursive: true });

const c = (section, extra = "") => `#/c/${company}/${section}${extra}`;
const click = (page, text, tag = "button,a,[role=tab]") =>
  page.evaluate((text, tag) => {
    const el = [...document.querySelectorAll(tag)].find((e) => e.textContent.trim().startsWith(text));
    if (!el) throw new Error("no element: " + text);
    el.click();
  }, text, tag);
// Real mouse click on the first leaf element whose text starts with `text`.
const press = async (page, text) => {
  const pt = await page.evaluate((t) => {
    const el = [...document.querySelectorAll("main *")].find((e) => e.children.length === 0 && e.textContent.trim().startsWith(t));
    if (!el) throw new Error("no element: " + t);
    el.scrollIntoView({ block: "center" });
    const r = el.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  }, text);
  await page.mouse.click(pt.x, pt.y);
};
const scrollTo = (page, text) => page.evaluate((t) => [...document.querySelectorAll("main h2,main h3")].find((e) => e.textContent.trim().startsWith(t))?.scrollIntoView({ block: "start" }), text).then(() => page.evaluate(() => window.scrollBy(0, -110)));
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// slug, hash, optional prep(page). `mobile: true` also yields a phone shot.
const SHOTS = [
  { slug: "oversikt", hash: c("oversikt"), mobile: true },
  { slug: "faktura", hash: c("faktura"), mobile: true },
  { slug: "hovedbok", hash: c("hovedbok"), prep: async (p) => { await press(p, "Kontoplan"); await wait(500); await press(p, "1920"); } },
  { slug: "bilag", hash: c("bilag"), prep: (p) => press(p, "skyhost") },
  { slug: "mva", hash: c("mva") },
  { slug: "bank", hash: c("bank") },
  { slug: "revisjon", hash: c("rapporter"), prep: (p) => click(p, "Revisjon") },
  { slug: "lonn", hash: c("lonn") },
  { slug: "timer", hash: c("timer") },
  { slug: "byra", hash: `#/byra/${firm}` },
  { slug: "integrasjoner", hash: c("admin", "/brukere"), prep: (p) => scrollTo(p, "Agenter og integrasjoner") },
];

const browser = await puppeteer.launch({
  executablePath: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  headless: true,
});

async function shoot(shot, theme, mobile) {
  const page = await browser.newPage();
  await page.setViewport(mobile ? { width: 390, height: 844, deviceScaleFactor: 2, isMobile: true, hasTouch: true } : { width: 1440, height: 900, deviceScaleFactor: 2 });
  await page.evaluateOnNewDocument((theme) => localStorage.setItem("regnmed-theme", theme), theme);
  await page.setRequestInterception(true);
  page.on("request", async (r) => {
    const url = new URL(r.url());
    if (url.pathname === "/auth/refresh")
      return r.respond({ status: 200, contentType: "application/json", body: JSON.stringify({ access_token: token, token_type: "Bearer", expires_in: 3600 }) });
    // The seed identity is mirrored from the token ("Demo Bruker", a test.invalid address); show a
    // plausible person instead by rewriting JSON the API returns. Reads only; nothing is stored.
    if (r.method() === "GET" && url.origin === base && /^\/(me|companies|firms|platform)\b/.test(url.pathname)) {
      const res = await fetch(r.url(), { headers: { authorization: `Bearer ${token}` } });
      const type = res.headers.get("content-type") ?? "";
      if (type.includes("json")) {
        const body = (await res.text()).replaceAll("Demo Bruker", "Ingrid Solberg").replace(/browser[.|][0-9a-f-]+@test\.invalid/g, "ingrid.solberg@fjordvik.example");
        return r.respond({ status: res.status, contentType: type, body });
      }
    }
    return r.continue();
  });
  await page.goto(base + "/" + shot.hash, { waitUntil: "networkidle2" });
  await wait(600);
  await page.evaluate(() => [...document.querySelectorAll(".alert")].filter((e) => e.textContent.includes("Prøvetid")).forEach((e) => e.remove()));
  if (shot.prep && !mobile) { await shot.prep(page); await page.waitForNetworkIdle({ idleTime: 500, concurrency: 1 }); await wait(400); }
  const name = `${shot.slug}${mobile ? "-mobile" : ""}-${theme === "light" ? "light" : "dark"}.png`;
  await page.screenshot({ path: `${out}/${name}` });
  console.log("wrote", name);
  await page.close();
}

for (const shot of SHOTS) {
  if (only && !only.split(",").includes(shot.slug)) continue;
  for (const theme of ["light", "steelzombie"]) {
    await shoot(shot, theme, false);
    if (shot.mobile && theme === "light") await shoot(shot, theme, true);
  }
}
await browser.close();
