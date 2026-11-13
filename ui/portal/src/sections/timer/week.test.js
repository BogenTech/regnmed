import { test } from "node:test";
import assert from "node:assert/strict";
import { addDays, mondayOf, isoWeek, weekTitle, dayName, parseHours } from "./week.js";

test("Monday of a week, across month and year", () => {
  assert.equal(mondayOf("2026-11-13"), "2026-11-09");
  assert.equal(mondayOf("2026-11-09"), "2026-11-09");
  assert.equal(mondayOf("2026-11-15"), "2026-11-09");
  assert.equal(mondayOf("2026-01-01"), "2025-12-29");
  assert.equal(addDays("2026-02-28", 1), "2026-03-01");
});

test("ISO week numbers, including week 53 and week 1 in December", () => {
  assert.equal(isoWeek("2026-11-09"), 46);
  assert.equal(isoWeek("2026-01-01"), 1);
  assert.equal(isoWeek("2020-12-31"), 53);
  assert.equal(isoWeek("2024-12-30"), 1);
});

test("week titles name what the week spans", () => {
  assert.equal(weekTitle("2026-11-09"), "Uke 46 · 9.–15. november 2026");
  assert.equal(weekTitle("2026-06-29"), "Uke 27 · 29. juni – 5. juli 2026");
  assert.equal(weekTitle("2025-12-29"), "Uke 1 · 29. desember 2025 – 4. januar 2026");
});

test("day names for screen readers", () => {
  assert.equal(dayName("2026-11-09"), "mandag 9. november");
  assert.equal(dayName("2026-11-15"), "søndag 15. november");
});

test("hours parse from comma, point and clock forms", () => {
  assert.equal(parseHours("7,5"), 450);
  assert.equal(parseHours("7.5"), 450);
  assert.equal(parseHours("7:30"), 450);
  assert.equal(parseHours(" "), 0);
  assert.equal(parseHours("0"), 0);
  assert.throws(() => parseHours("abc"));
  assert.throws(() => parseHours("-1"));
  assert.throws(() => parseHours("25"));
  assert.throws(() => parseHours("7:75"));
});
