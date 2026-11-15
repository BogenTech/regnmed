import { test } from "node:test";
import assert from "node:assert/strict";
import { forrigeManedsslutt, kanIkkeAvslutte, laseForslag, lesSkatt } from "./periode.js";

test("forrigeManedsslutt crosses months and years", () => {
  assert.equal(forrigeManedsslutt("2026-11-15"), "2026-10-31");
  assert.equal(forrigeManedsslutt("2026-03-01"), "2026-02-28");
  assert.equal(forrigeManedsslutt("2024-03-31"), "2024-02-29");
  assert.equal(forrigeManedsslutt("2026-01-10"), "2025-12-31");
});

test("laseForslag only suggests a lock that moves forward", () => {
  assert.equal(laseForslag(null, "2026-11-15"), "2026-10-31");
  assert.equal(laseForslag("2026-09-30", "2026-11-15"), "2026-10-31");
  assert.equal(laseForslag("2026-10-31", "2026-11-15"), null);
  assert.equal(laseForslag("2026-11-10", "2026-11-15"), null);
});

test("lesSkatt reads amounts the way people type them", () => {
  assert.deepEqual(lesSkatt("10 000"), { ore: 1_000_000 });
  assert.deepEqual(lesSkatt("13 200,50"), { ore: 1_320_050 });
  assert.deepEqual(lesSkatt("0"), { ore: 0 });
  assert.match(lesSkatt("").feil, /0 er et gyldig svar/);
  assert.match(lesSkatt("abc").feil, /beløp/);
  assert.match(lesSkatt("-5").feil, /negativ/);
});

test("kanIkkeAvslutte names why a year cannot be closed", () => {
  const rader = [{ ar: 2024, bilag: "2024-31" }];
  assert.equal(kanIkkeAvslutte(2025, "2026-05-01", rader, "2026-03-31", true), null);
  assert.match(kanIkkeAvslutte(2026, "2026-11-15", rader, null, true), /ikke over ennå/);
  assert.match(kanIkkeAvslutte(2024, "2026-05-01", rader, null, true), /avsluttet med bilag 2024-31/);
  assert.match(kanIkkeAvslutte(2025, "2026-05-01", rader, "2026-03-31", false), /administrator/);
  assert.equal(kanIkkeAvslutte(2025, "2026-05-01", rader, "2025-10-31", false), null);
});
