import { test } from "node:test";
import assert from "node:assert/strict";
import { oppdragstype, delKlienter } from "./byra.js";

test("oppdragstype names the stored kinds", () => {
  assert.equal(oppdragstype("regnskap"), "Regnskapsføring");
  assert.equal(oppdragstype("revisjon"), "Revisjon");
  assert.equal(oppdragstype("annet"), "annet");
});

test("delKlienter splits active from ended, by name, valid_to exclusive", () => {
  const { aktive, avsluttede } = delKlienter(
    [
      { company: "Øst AS", valid_to: null },
      { company: "Alfa AS", valid_to: "2026-11-16" },
      { company: "Beta AS", valid_to: "2026-12-01" },
      { company: "Ågot AS", valid_to: null },
    ],
    "2026-11-16",
  );
  assert.deepEqual(
    aktive.map((e) => e.company),
    ["Beta AS", "Øst AS", "Ågot AS"],
  );
  assert.deepEqual(
    avsluttede.map((e) => e.company),
    ["Alfa AS"],
  );
});
