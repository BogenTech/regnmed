import { today } from "../../lib/format.js";

// The employment details the a-melding requires (F290/F291, migration
// 0073). The form holds text; tilPayload turns it into what the API takes.
// A blank field means not given. Suggestions are filled in visibly and can
// always be changed: what is saved is what the user sees.
export function tomt() {
  return {
    virksomhet: "",
    yrkeskode: "",
    stillingsprosent: "",
    timer: "",
    arbeidstidsordning: "",
    ansettelsesform: "",
    sisteLonnsendring: "",
  };
}

const hundredeler = (n) => (n == null ? "" : String(n / 100).replace(".", ","));

export function fraAnsatt(a) {
  const f = a.arbeidsforhold || {};
  return {
    virksomhet: f.virksomhet_orgnr || "",
    yrkeskode: f.yrkeskode || "",
    stillingsprosent: hundredeler(f.stillingsprosent_bp),
    timer: hundredeler(f.timer_per_uke_hundredeler),
    arbeidstidsordning: f.arbeidstidsordning || "",
    ansettelsesform: f.ansettelsesform || "",
    sisteLonnsendring: f.siste_lonnsendring || "",
  };
}

const current = (e, date) => !e.ansatt_til || e.ansatt_til >= date;

// Employees whose rate says something about a new hire: still employed,
// and not at the age where the over-60 addition is earned.
export const rateEmployees = (employees, date = today()) =>
  employees.filter(
    (e) =>
      current(e, date) &&
      (!e.fodselsdato || +e.fodselsdato.slice(0, 4) > +date.slice(0, 4) - 59),
  );

// The rate most of those employees have, the lowest on a tie.
export function commonRate(employees, date = today()) {
  const counts = new Map();
  for (const e of rateEmployees(employees, date)) {
    counts.set(e.feriepenger_bp, (counts.get(e.feriepenger_bp) || 0) + 1);
  }
  let best = 1020;
  let most = 0;
  for (const [bp, n] of counts) {
    if (n > most || (n === most && bp < best)) {
      best = bp;
      most = n;
    }
  }
  return hundredeler(best);
}

// Terms from the latest current employee with complete details. The
// occupation is personal and never copied. The start date is only known
// for a new hire.
export function suggest(employees, units, startDate = "", date = today()) {
  const latest = employees
    .filter((e) => current(e, date) && !e.amelding_mangler?.length)
    .reduce((last, e) => (!last || e.ansatt_fra > last.ansatt_fra ? e : last), null);
  const f = latest ? fraAnsatt(latest) : tomt();
  const known = !units.length || units.some((u) => u.orgnumber === f.virksomhet);
  return {
    virksomhet: units.length === 1 ? units[0].orgnumber : known ? f.virksomhet : "",
    yrkeskode: "",
    stillingsprosent: f.stillingsprosent || "100",
    timer: f.timer,
    arbeidstidsordning: f.arbeidstidsordning || "ikkeSkift",
    ansettelsesform: f.ansettelsesform || "fast",
    sisteLonnsendring: startDate,
  };
}

export const fillBlanks = (value, f) =>
  Object.fromEntries(Object.entries(value).map(([field, v]) => [field, v || f[field]]));

const tall = (s) => (s.trim() ? Math.round(parseFloat(s.trim().replace(",", ".")) * 100) : null);
const tekst = (s) => s.trim() || null;

// Without a skattekort the employer must withhold 50 %, so that is what a
// new hire carries until the card arrives.
const NO_CARD_BP = 5000;

export const newWithholding = (canFetch) => ({
  kind: canFetch ? "card" : "prosent",
  percent: canFetch ? "" : hundredeler(NO_CARD_BP),
  table: "",
});

// A frikort or 12-month card cannot be set by hand, so it starts blank.
export function withholdingOf(a) {
  if (a.trekk_type === "tabell") return { kind: "tabell", percent: "", table: String(a.trekk_tabell) };
  if (a.trekk_type === "ingen") return { kind: "ingen", percent: "", table: "" };
  const byHand = a.trekk_type === "prosent" && !a.trekk_12_maneder;
  return { kind: "prosent", percent: byHand ? hundredeler(a.trekk_prosent_bp) : "", table: "" };
}

export function withholdingPayload(v) {
  const none = { trekk_prosent_bp: null, trekk_tabell: null };
  if (v.kind === "card") return { ...none, trekk_type: "prosent", trekk_prosent_bp: NO_CARD_BP };
  if (v.kind === "prosent") return { ...none, trekk_type: "prosent", trekk_prosent_bp: tall(v.percent) };
  if (v.kind === "tabell") {
    const n = v.table.trim();
    return { ...none, trekk_type: "tabell", trekk_tabell: /^\d+$/.test(n) ? Number(n) : null };
  }
  return { ...none, trekk_type: "ingen" };
}

// Orders are newest first. An order still waiting for its answer stays
// visible unless a newer answered order covered it: one of at least its
// size, when that is a company-wide fetch and not another single hire.
export function openCardOrders(orders, year) {
  return orders.filter(
    (o, i) =>
      o.inntektsaar === year &&
      o.referanse &&
      !o.besvart &&
      !o.feil &&
      !orders
        .slice(0, i)
        .some(
          (n) =>
            n.inntektsaar === year &&
            n.besvart &&
            n.antall_ansatte > 1 &&
            n.antall_ansatte >= o.antall_ansatte,
        ),
  );
}

// Each employee's newest card for the year, across single and full orders.
export function newestCards(orders, year) {
  const cards = new Map();
  for (const o of orders) {
    if (o.inntektsaar !== year) continue;
    for (const c of o.skattekort) if (!cards.has(c.ansatt_id)) cards.set(c.ansatt_id, c);
  }
  return [...cards.values()];
}

export function tilPayload(v) {
  return {
    virksomhet_orgnr: tekst(v.virksomhet.replace(/\s/g, "")),
    yrkeskode: tekst(v.yrkeskode),
    stillingsprosent_bp: tall(v.stillingsprosent),
    timer_per_uke_hundredeler: tall(v.timer),
    arbeidstidsordning: tekst(v.arbeidstidsordning),
    ansettelsesform: tekst(v.ansettelsesform),
    siste_lonnsendring: tekst(v.sisteLonnsendring),
  };
}
