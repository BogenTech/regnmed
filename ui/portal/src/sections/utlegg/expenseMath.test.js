import { test } from "node:test";
import assert from "node:assert/strict";
import { mileage, parseKm } from "./expenseMath.js";

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
