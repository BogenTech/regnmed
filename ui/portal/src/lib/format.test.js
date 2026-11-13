import { test } from "node:test";
import assert from "node:assert/strict";
import { krWhole, formatDate, today } from "./format.js";

test("krWhole rounds half away from zero and keeps the sign", () => {
  assert.equal(krWhole(1375040), "13 750");
  assert.equal(krWhole(150), "2");
  assert.equal(krWhole(-150), "−2");
  assert.equal(krWhole(-40), "0");
  assert.equal(krWhole(-29540400), "−295 404");
});

test("formatDate turns ISO dates and timestamps into dd.mm.yyyy", () => {
  assert.equal(formatDate("2026-06-20"), "20.06.2026");
  assert.equal(formatDate("2026-12-10T08:15:00Z"), "10.12.2026");
  assert.equal(formatDate("åpen"), "åpen");
  assert.equal(formatDate(null), null);
});

test("today is the local date, not UTC's", () => {
  // 00:30 local; UTC may still be the day before.
  const lateNight = new Date(2026, 10, 13, 0, 30);
  assert.equal(today(lateNight), "2026-11-13");
});

test("amounts are read the way they are typed, spaces and all", async () => {
  const { oreOrNull } = await import("./format.js");
  assert.equal(oreOrNull("12 500,00"), 1_250_000);
  assert.equal(oreOrNull("12 500"), 1_250_000);
  assert.equal(oreOrNull("1250.5"), 125_050);
  assert.equal(oreOrNull("−45,50"), -4_550);
  assert.equal(oreOrNull(""), null);
  assert.equal(oreOrNull("tolv"), null);
});
