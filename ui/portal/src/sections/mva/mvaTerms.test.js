import { test } from "node:test";
import assert from "node:assert/strict";
import { termName, defaultTerm, deadlineText } from "./mvaTerms.js";

test("terms are named by their months", () => {
  assert.equal(termName(6, 1), "Jan–feb");
  assert.equal(termName(6, 6), "Nov–des");
  assert.equal(termName(1, 1), "Hele året");
  assert.equal(termName(12, 3), "Mar");
});

test("the page opens on the latest ENDED term", () => {
  assert.deepEqual(defaultTerm(6, "2026-11-13"), { year: 2026, termin: 5 });
  assert.deepEqual(defaultTerm(6, "2026-01-20"), { year: 2025, termin: 6 });
  assert.deepEqual(defaultTerm(6, "2026-03-01"), { year: 2026, termin: 1 });
  assert.deepEqual(defaultTerm(1, "2026-02-01"), { year: 2025, termin: 1 });
});

test("deadlines are said as a distance", () => {
  assert.equal(deadlineText("2026-12-10", "2026-11-13"), "om 27 dager");
  assert.equal(deadlineText("2026-11-13", "2026-11-13"), "i dag");
  assert.equal(deadlineText("2026-11-14", "2026-11-13"), "i morgen");
  assert.equal(deadlineText("2026-04-10", "2026-11-13"), "passert for 217 dager siden");
});
