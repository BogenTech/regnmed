import test from "node:test";
import assert from "node:assert/strict";
import { strictOre, featuresFromText, planBody, priceBody, currentAndNext } from "./planer.js";

test("strictOre reads kroner as text and rejects everything that would be rounded", () => {
  assert.equal(strictOre("49"), 4900);
  assert.equal(strictOre("49,50"), 4950);
  assert.equal(strictOre("49.5"), 4950);
  assert.equal(strictOre("1 299"), 129900);
  assert.equal(strictOre("1 299,00"), 129900);
  assert.equal(strictOre("0"), 0);
  assert.equal(strictOre(""), null);
  assert.equal(strictOre("  "), null);
  assert.equal(strictOre("-1"), null);
  assert.equal(strictOre("49,555"), null);
  assert.equal(strictOre("49,"), null);
  assert.equal(strictOre("tolv"), null);
  assert.equal(strictOre("9".repeat(20)), null);
});

test("no float drift: 0,07 and 19,99 stay exact", () => {
  assert.equal(strictOre("0,07"), 7);
  assert.equal(strictOre("19,99"), 1999);
  assert.equal(strictOre("1.15"), 115);
});

test("features come one per line, blanks dropped", () => {
  assert.deepEqual(featuresFromText(" a \n\n b\n"), ["a", "b"]);
  assert.deepEqual(featuresFromText(""), []);
});

test("planBody: empty limit means no limit, junk is refused", () => {
  const ok = planBody({ name: " Liten ", voucher_limit: "1 500", employee_limit: "", features: "x", listed: true });
  assert.deepEqual(ok, { kind: "plan", available: true, name: "Liten", audience: "", featured: false, listed: true, voucher_limit: 1500, employee_limit: null, features: ["x"], sort: 0 });
  assert.throws(() => planBody({ name: "" }), /navn/);
  assert.throws(() => planBody({ name: "x", voucher_limit: "1,5" }), /helt tall/);
  assert.throws(() => planBody({ name: "x", sort: "1.5" }), /rekkefølge/);
});

test("priceBody refuses past dates, bad amounts and missing source", () => {
  const v = { interval: "month", price: "129", valid_from: "2026-12-01", source: "vedtak 2026-11-20" };
  assert.deepEqual(priceBody(v, "2026-11-18"), { interval: "month", price_ore: 12900, valid_from: "2026-12-01", source: "vedtak 2026-11-20" });
  assert.equal(priceBody({ ...v, valid_from: "2026-11-18" }, "2026-11-18").valid_from, "2026-11-18");
  assert.throws(() => priceBody({ ...v, valid_from: "2026-11-17" }, "2026-11-18"), /bakover/);
  assert.throws(() => priceBody({ ...v, price: "129,999" }, "2026-11-18"), /beløp/);
  assert.throws(() => priceBody({ ...v, source: " " }, "2026-11-18"), /kilden/);
  assert.throws(() => priceBody({ ...v, valid_from: "" }, "2026-11-18"), /dato/);
});

test("currentAndNext picks the price in force and the earliest future one", () => {
  const prices = [
    { interval: "month", price_ore_per_month: 9, valid_from: "2027-03-01" },
    { interval: "month", price_ore_per_month: 8, valid_from: "2027-01-01" },
    { interval: "year", price_ore_per_month: 100, valid_from: "2026-01-01" },
    { interval: "month", price_ore_per_month: 7, valid_from: "2026-07-01" },
    { interval: "month", price_ore_per_month: 6, valid_from: "2026-01-01" },
  ];
  const r = currentAndNext(prices, "month", "2026-11-18");
  assert.equal(r.current.price_ore_per_month, 7);
  assert.equal(r.next.price_ore_per_month, 8);
  assert.equal(currentAndNext(prices, "month", "2026-07-01").current.price_ore_per_month, 7);
  assert.deepEqual(currentAndNext([], "month", "2026-11-18"), { current: null, next: null });
  assert.equal(currentAndNext(prices, "year", "2026-11-18").next, null);
});

test("planBody: only add-ons can be unavailable", () => {
  assert.equal(planBody({ name: "Bank", kind: "addon", available: false }).available, false);
  assert.equal(planBody({ name: "Bank", kind: "addon", available: true }).available, true);
  assert.equal(planBody({ name: "Liten", available: false }).available, true);
  assert.equal(planBody({ name: "Liten", kind: "junk" }).kind, "plan");
});
