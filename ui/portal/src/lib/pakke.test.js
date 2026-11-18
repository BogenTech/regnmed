import test from "node:test";
import assert from "node:assert/strict";
import { naadd, bruktekst, minsteSomPasser } from "./pakke.js";

const planer = [
  { plan: "stor", pris_ore_per_mnd: 29900, bilag_grense: 10000, ansatte_grense: 25, integrasjoner_grense: null },
  { plan: "liten", pris_ore_per_mnd: 4900, bilag_grense: 150, ansatte_grense: 1, integrasjoner_grense: 0 },
  { plan: "middels", pris_ore_per_mnd: 12900, bilag_grense: 1500, ansatte_grense: 5, integrasjoner_grense: 3 },
];

test("the cheapest package that fits every limit is suggested", () => {
  assert.equal(minsteSomPasser(planer, { bilag: 40, ansatte: 1, integrasjoner: 0 }).plan, "liten");
  assert.equal(minsteSomPasser(planer, { bilag: 151, ansatte: 0, integrasjoner: 0 }).plan, "middels");
  assert.equal(minsteSomPasser(planer, { bilag: 10, ansatte: 0, integrasjoner: 1 }).plan, "middels", "an integration rules out Liten");
  assert.equal(minsteSomPasser(planer, { bilag: 10, ansatte: 6, integrasjoner: 0 }).plan, "stor");
  assert.equal(minsteSomPasser(planer, { bilag: 20000, ansatte: 0, integrasjoner: 0 }), null, "nothing fits");
  assert.equal(minsteSomPasser(planer, null), null);
});

test("a null limit is no limit", () => {
  assert.equal(naadd(5, null), false);
  assert.equal(naadd(1, 1), true);
  assert.equal(naadd(0, 0), true, "a limit of 0 is reached at once");
  assert.equal(naadd(0, 1), false);
});

test("usage text", () => {
  assert.equal(bruktekst(312, 150, "bilag i år"), "312 av 150 bilag i år");
  assert.equal(bruktekst(1200, null, "bilag i år"), "1\u00a0200 bilag i år");
});
