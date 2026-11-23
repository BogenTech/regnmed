import test from "node:test";
import assert from "node:assert/strict";
import {
  commonRate,
  fillBlanks,
  newWithholding,
  suggest,
  tomt,
  newestCards,
  openCardOrders,
  withholdingOf,
  withholdingPayload,
} from "./employment.js";

const ansatt = (fra, unit, bp = 1020, mangler = []) => ({
  ansatt_fra: fra,
  feriepenger_bp: bp,
  amelding_mangler: mangler,
  arbeidsforhold: {
    virksomhet_orgnr: unit,
    yrkeskode: "2512101",
    stillingsprosent_bp: 8000,
    timer_per_uke_hundredeler: 3750,
    arbeidstidsordning: "andreSkift",
    ansettelsesform: "midlertidig",
    siste_lonnsendring: "2025-01-01",
  },
});

test("the holiday pay suggestion is the company's most common rate", () => {
  assert.equal(commonRate([]), "10,2");
  assert.equal(commonRate([ansatt("2026-01-01", "1", 1200), ansatt("2026-02-01", "1", 1200), ansatt("2026-03-01", "1", 1250)]), "12");
  assert.equal(commonRate([ansatt("2026-01-01", "1", 1250), ansatt("2026-02-01", "1", 1020)]), "10,2");
});

test("a first employee gets the plain defaults and the only business unit", () => {
  const f = suggest([], [{ orgnumber: "315286913" }], "2026-11-23");
  assert.deepEqual(f, {
    virksomhet: "315286913",
    yrkeskode: "",
    stillingsprosent: "100",
    timer: "",
    arbeidstidsordning: "ikkeSkift",
    ansettelsesform: "fast",
    sisteLonnsendring: "2026-11-23",
  });
});

test("terms come from the latest complete employee, never the occupation", () => {
  const list = [ansatt("2024-01-01", "111"), ansatt("2026-01-01", "222"), ansatt("2026-06-01", "333", 1020, ["yrke"])];
  const f = suggest(list, [{ orgnumber: "111" }, { orgnumber: "222" }], "2026-11-23");
  assert.equal(f.virksomhet, "222");
  assert.equal(f.stillingsprosent, "80");
  assert.equal(f.timer, "37,5");
  assert.equal(f.ansettelsesform, "midlertidig");
  assert.equal(f.yrkeskode, "");
});

test("a business unit no longer in the register is not suggested", () => {
  const f = suggest([ansatt("2026-01-01", "999")], [{ orgnumber: "111" }, { orgnumber: "222" }]);
  assert.equal(f.virksomhet, "");
});

test("suggestions never overwrite what an employee already has", () => {
  const v = { ...tomt(), stillingsprosent: "50", sisteLonnsendring: "2025-05-01" };
  const filled = fillBlanks(v, suggest([], [{ orgnumber: "315286913" }]));
  assert.equal(filled.stillingsprosent, "50");
  assert.equal(filled.sisteLonnsendring, "2025-05-01");
  assert.equal(filled.virksomhet, "315286913");
  assert.equal(filled.ansettelsesform, "fast");
});

test("an existing employee's last pay change is not guessed", () => {
  assert.equal(fillBlanks(tomt(), suggest([], [])).sisteLonnsendring, "");
});

test("the rate ignores leavers and employees at the over-60 age", () => {
  const leaver = { ...ansatt("2024-01-01", "1", 1200), ansatt_til: "2026-10-31" };
  const senior = { ...ansatt("2024-01-01", "1", 1250), fodselsdato: "1966-03-01" };
  const young = { ...ansatt("2025-01-01", "1", 1020), fodselsdato: "1990-03-01" };
  assert.equal(commonRate([leaver, leaver, senior, senior, young], "2026-11-23"), "10,2");
  assert.equal(commonRate([senior], "2026-11-23"), "10,2");
});

test("a leaver's terms are not suggested", () => {
  const leaver = { ...ansatt("2026-05-01", "111"), ansatt_til: "2026-10-31" };
  assert.equal(suggest([leaver], [], "2026-11-23", "2026-11-23").stillingsprosent, "100");
});

test("withholding becomes what the API takes", () => {
  assert.deepEqual(withholdingPayload(newWithholding(true)), {
    trekk_type: "prosent", trekk_prosent_bp: 5000, trekk_tabell: null,
  });
  assert.equal(newWithholding(false).percent, "50");
  assert.deepEqual(withholdingPayload({ kind: "tabell", percent: "", table: " 8010 " }), {
    trekk_type: "tabell", trekk_prosent_bp: null, trekk_tabell: 8010,
  });
  assert.equal(withholdingPayload({ kind: "tabell", percent: "", table: "80x" }).trekk_tabell, null);
  assert.equal(withholdingPayload({ kind: "prosent", percent: "35,5", table: "" }).trekk_prosent_bp, 3550);
  assert.equal(withholdingPayload({ kind: "ingen", percent: "9", table: "9" }).trekk_prosent_bp, null);
});

test("a card-only withholding is not carried into the manual form", () => {
  assert.equal(withholdingOf({ trekk_type: "frikort" }).percent, "");
  assert.equal(withholdingOf({ trekk_type: "prosent", trekk_prosent_bp: 2220, trekk_12_maneder: true }).percent, "");
  assert.equal(withholdingOf({ trekk_type: "prosent", trekk_prosent_bp: 3800 }).percent, "38");
  assert.equal(withholdingOf({ trekk_type: "tabell", trekk_tabell: 9040 }).table, "9040");
});

test("a single hire's order does not hide a company-wide order or older cards", () => {
  const order = (id, count, cards, extra = {}) => ({
    id, inntektsaar: 2026, referanse: "BR" + id, antall_ansatte: count,
    besvart: cards.length > 0, feil: null, skattekort: cards, ...extra,
  });
  const single = order("3", 1, [{ ansatt_id: "new", satt: "prosent" }]);
  const open = order("2", 4, []);
  const old = order("1", 4, [{ ansatt_id: "new", satt: null }, { ansatt_id: "kept", merknad: "n" }]);
  assert.deepEqual(openCardOrders([single, open, old], 2026).map((o) => o.id), ["2"]);
  const cards = newestCards([single, open, old], 2026);
  assert.equal(cards.length, 2);
  assert.equal(cards.find((c) => c.ansatt_id === "new").satt, "prosent");
  assert.equal(cards.find((c) => c.ansatt_id === "kept").merknad, "n");
});

test("a later full fetch retires an open order, a later single hire does not", () => {
  const order = (id, count, answered) => ({
    id, inntektsaar: 2026, referanse: "BR" + id, antall_ansatte: count,
    besvart: answered, feil: null, skattekort: [],
  });
  assert.deepEqual(openCardOrders([order("2", 4, true), order("1", 4, false)], 2026), []);
  assert.deepEqual(
    openCardOrders([order("2", 1, true), order("1", 1, false)], 2026).map((o) => o.id),
    ["1"],
  );
  assert.deepEqual(openCardOrders([order("1", 4, false)], 2025), []);
});
