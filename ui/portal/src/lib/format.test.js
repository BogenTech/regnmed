import { test } from "node:test";
import assert from "node:assert/strict";
import { krWhole, formatDate, formatTime, today, kr, parseKr } from "./format.js";

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

test("parseKr reads back what kr writes, negatives included", () => {
  for (const ore of [0, 5, 125000, -5000, -123456789]) assert.equal(parseKr(kr(ore)), ore);
});

test("formatTime shows a UTC timestamp in the reader's time zone", () => {
  const tz = process.env.TZ;
  process.env.TZ = "Europe/Oslo";
  try {
    assert.equal(formatTime("2026-11-14T23:30:00+00:00"), "15.11.2026 00:30");
    assert.equal(formatTime("2026-07-01T06:05:00Z"), "01.07.2026 08:05");
    assert.equal(formatTime(null), "");
    assert.equal(formatTime("ikke en tid"), "ikke en tid");
  } finally {
    if (tz === undefined) delete process.env.TZ;
    else process.env.TZ = tz;
  }
});
