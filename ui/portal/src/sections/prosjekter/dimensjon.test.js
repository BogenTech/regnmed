import { test } from "node:test";
import assert from "node:assert/strict";
import { delOpp, lesSats } from "./dimensjon.js";

test("delOpp splits one kind into open and closed", () => {
  const dims = [
    { kind: "prosjekt", code: "P1", active: true },
    { kind: "prosjekt", code: "P2", active: false },
    { kind: "avdeling", code: "A1", active: true },
  ];
  const { aktive, avsluttede } = delOpp(dims, "prosjekt");
  assert.deepEqual(
    aktive.map((d) => d.code),
    ["P1"],
  );
  assert.deepEqual(
    avsluttede.map((d) => d.code),
    ["P2"],
  );
});

test("lesSats never turns an empty field into 0 kr", () => {
  assert.ok(lesSats("").feil);
  assert.ok(lesSats("   ").feil);
  assert.ok(lesSats(undefined).feil);
  assert.ok(lesSats("abc").feil);
  assert.ok(lesSats("-5").feil);
  assert.deepEqual(lesSats("0"), { ore: 0 });
  assert.deepEqual(lesSats("1 250,50"), { ore: 125050 });
});
