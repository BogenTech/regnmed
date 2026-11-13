import { test } from "node:test";
import assert from "node:assert/strict";
import { buildAddress, parseAddress } from "./reportAddress.js";

test("reads tab, year and the tab's own params", () => {
  assert.deepEqual(parseAddress("kontospesifikasjon", "year=2025&konto=1920&side=3", 2026), {
    tab: "kontospesifikasjon",
    year: 2025,
    params: { konto: "1920", side: "3" },
  });
});

test("falls back to saldobalanse and this year on anything unknown", () => {
  assert.deepEqual(parseAddress("nope", "year=abc", 2026), {
    tab: "saldobalanse",
    year: 2026,
    params: {},
  });
  assert.equal(parseAddress(null, "", 2026).tab, "saldobalanse");
});

test("drops empty params and round-trips", () => {
  const hash = buildAddress("c1", "resultat", 2026, { avdeling: "", prosjekt: "P1", side: undefined });
  assert.equal(hash, "#/c/c1/rapporter/resultat?year=2026&prosjekt=P1");
  const [tab, query] = hash.split("/rapporter/")[1].split("?");
  assert.deepEqual(parseAddress(tab, query, 2000), {
    tab: "resultat",
    year: 2026,
    params: { prosjekt: "P1" },
  });
});
