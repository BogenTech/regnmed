// The price list shown on www, read from the platform console's data at build time.
// A failure to reach the API falls back to data/prices.js with a loud warning, so a
// deploy never breaks on an API blip; the site rebuilds on a schedule (site.yml).
import { FALLBACK } from "./prices.js";

const URL = process.env.PRICING_URL ?? "https://api.regnmed.no/pricing";

const valid = (j) => Array.isArray(j?.plans) && Array.isArray(j?.addons) && j.plans.every((p) => typeof p.slug === "string" && typeof p.name === "string");

export async function loadPricing() {
  try {
    const res = await fetch(URL, { signal: AbortSignal.timeout(8000) });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const json = await res.json();
    if (!valid(json)) throw new Error("unexpected shape");
    return { ...json, source: "api" };
  } catch (e) {
    const msg = `[pricing] using the built-in fallback (the prisvedtak in force), ${URL} failed: ${e.message}`;
    // A GitHub annotation, so a build that fell back is visible on the run instead of in a log nobody reads.
    console.warn(process.env.GITHUB_ACTIONS ? `::warning title=Pricing fallback::${msg}` : msg);
    return { ...FALLBACK, source: "fallback" };
  }
}

// Whole kroner without decimals, anything else with two ("29,50"); works from integer øre.
const kr = (ore) =>
  new Intl.NumberFormat("nb-NO", ore % 100 === 0 ? { maximumFractionDigits: 0 } : { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(ore / 100);
export const fmtKr = kr;
export const fmtN = (n) => new Intl.NumberFormat("nb-NO").format(n);
// The lowest price in force today; announced prices are not for sale yet.
export const fromPrice = (plans) => {
  const prices = plans.map((p) => p.price_ore_per_month).filter((x) => x != null);
  return prices.length ? Math.min(...prices) : null;
};
// "49 kr/mnd", "129 kr/mnd fra 1. februar 2027", or "Pris kommer".
export function priceLabel(p) {
  if (p.price_ore_per_month != null) return { main: kr(p.price_ore_per_month), unit: " kr/mnd per selskap", note: p.upcoming ? `${kr(p.upcoming.price_ore_per_month)} kr fra ${date(p.upcoming.valid_from)}` : null };
  if (p.upcoming) return { main: kr(p.upcoming.price_ore_per_month), unit: " kr/mnd per selskap", note: p.upcoming.valid_from ? `gjelder fra ${date(p.upcoming.valid_from)}` : "innføres snart" };
  return { main: "Pris kommer", unit: "", note: null };
}
const date = (iso) => (iso ? new Date(iso + "T00:00:00").toLocaleDateString("nb-NO", { day: "numeric", month: "long", year: "numeric" }) : "");
