import { test } from "node:test";
import assert from "node:assert/strict";
import { nextPeriod, slipFilename, startingZone } from "./periode.js";

test("the form offers the month after the latest run", () => {
  const runs = [
    { ar: 2026, maned: 5 },
    { ar: 2026, maned: 6 },
  ];
  assert.deepEqual(nextPeriod(runs, "2026-11-14"), { year: 2026, month: 7 });
});

test("December rolls over into January of the next year", () => {
  assert.deepEqual(nextPeriod([{ ar: 2025, maned: 12 }], "2026-01-10"), { year: 2026, month: 1 });
});

test("with no run yet, the current month", () => {
  assert.deepEqual(nextPeriod([], "2026-11-14"), { year: 2026, month: 11 });
});

test("a company's first run starts with no zone chosen", () => {
  assert.equal(startingZone([]), "");
});

test("a later run inherits the zone of the latest run", () => {
  const runs = [
    { ar: 2026, maned: 5, sone: "I" },
    { ar: 2026, maned: 10, sone: "IV" },
    { ar: 2025, maned: 12, sone: "II" },
  ];
  assert.equal(startingZone(runs), "IV");
});

test("payslip files are named by month and person", () => {
  assert.equal(slipFilename(2026, 6, "Kari Utvikler"), "lonnsslipp-2026-06-kari-utvikler.pdf");
  assert.equal(slipFilename(2026, 12, "Bjørn Ås-Lie"), "lonnsslipp-2026-12-bjorn-as-lie.pdf");
});
