import { test } from "node:test";
import assert from "node:assert/strict";
import { lesAntall, lesKonto, lesPris, mvaTekst, tellingLinjer, utvalg } from "./produkt.js";

test("lesPris never turns an empty field into 0 kr", () => {
  assert.ok(lesPris("").feil);
  assert.ok(lesPris("  ").feil);
  assert.ok(lesPris("abc").feil);
  assert.ok(lesPris("-5").feil);
  assert.deepEqual(lesPris("0"), { ore: 0 });
  assert.deepEqual(lesPris("1 250,50"), { ore: 125050 });
  assert.equal(lesPris("", "kostprisen").feil, "Skriv inn kostprisen i kroner");
});

test("lesKonto accepts only a four-digit class 3 account", () => {
  assert.deepEqual(lesKonto(" 3000 "), { konto: "3000" });
  for (const bad of ["", "9999", "300", "30000", "3a00", "1500"]) assert.ok(lesKonto(bad).feil, bad);
});

test("lesAntall reads Norwegian decimals and refuses zero unless counting", () => {
  assert.deepEqual(lesAntall("2,5"), { milli: 2500 });
  assert.deepEqual(lesAntall("−1"), { milli: -1000 });
  assert.ok(lesAntall("0").feil);
  assert.deepEqual(lesAntall("0", { tillatNull: true }), { milli: 0 });
  assert.ok(lesAntall("").feil);
  assert.ok(lesAntall("x").feil);
});

test("utvalg splits active from deactivated and searches number and name", () => {
  const p = [
    { nummer: "V1", navn: "Kaffekopp", aktiv: true },
    { nummer: "T1", navn: "Konsulenttime", aktiv: true },
    { nummer: "G1", navn: "Gammel kopp", aktiv: false },
  ];
  assert.deepEqual(utvalg(p, "").aktive.length, 2);
  const kopp = utvalg(p, "KOPP");
  assert.deepEqual(kopp.aktive.map((x) => x.nummer), ["V1"]);
  assert.deepEqual(kopp.deaktiverte.map((x) => x.nummer), ["G1"]);
  assert.deepEqual(utvalg(p, "t1").aktive.map((x) => x.nummer), ["T1"]);
});

test("mvaTekst shows the rate, not just the code", () => {
  assert.equal(mvaTekst("3"), "mva 25 %");
  assert.equal(mvaTekst(null), "uten mva");
  assert.equal(mvaTekst("99"), "mva-kode 99");
});

test("tellingLinjer keeps filled rows, allows zero, refuses negative and empty", () => {
  const inv = [{ nummer: "V1" }, { nummer: "V2" }, { nummer: "V3" }];
  assert.deepEqual(tellingLinjer(inv, { V1: "6,5", V2: "0", V3: " " }), {
    linjer: [
      { produkt: "V1", talt_milli: 6500 },
      { produkt: "V2", talt_milli: 0 },
    ],
  });
  assert.ok(tellingLinjer(inv, {}).feil);
  assert.match(tellingLinjer(inv, { V2: "-1" }).feil, /^V2:/);
});
