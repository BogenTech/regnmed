import { test } from "node:test";
import assert from "node:assert/strict";
import { mileage, nights, parseKm } from "./expenseMath.js";

test("mileage matches the core: 120 km at 5,30 with 3,50 tax-free", () => {
  assert.deepEqual(mileage(120, 530, 350), { amount: 63600, taxFree: 42000, taxable: 21600 });
});

test("the tax-free part never exceeds the amount", () => {
  assert.deepEqual(mileage(10, 300, 350), { amount: 3000, taxFree: 3000, taxable: 0 });
});

test("km are whole and positive, or nothing", () => {
  assert.equal(parseKm("120"), 120);
  assert.equal(parseKm(" 7 "), 7);
  for (const bad of ["", "0", "12,5", "12 km", "-3", null]) assert.equal(parseKm(bad), null, bad);
});


test("the døgn with a night follow the core's rule", () => {
  // Two full døgn and 6 hours: the third is the part døgn.
  assert.deepEqual(nights("2026-03-02T08:00", "2026-03-04T14:00", true, 3), [true, true, "optional"]);
  // A short trip with an overnight stay has one døgn, with its night.
  assert.deepEqual(nights("2026-03-02T18:00", "2026-03-03T10:00", true, 1), [true]);
  assert.deepEqual(nights("2026-03-02T08:00", "2026-03-02T20:00", false, 1), [false]);
  // The night between the 28th and 29th of March changes the clocks, and
  // still counts as a whole døgn.
  assert.deepEqual(nights("2026-03-28T08:00", "2026-03-29T08:00", true, 1), [true]);
});
