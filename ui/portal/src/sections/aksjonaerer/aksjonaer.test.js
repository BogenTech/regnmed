import { test } from "node:test";
import assert from "node:assert/strict";
import { adresseTekst, andelTekst, grupperTyper, hendelseTekst, identTekst, lesAksjer } from "./aksjonaer.js";

test("andelTekst uses a decimal comma", () => {
  assert.equal(andelTekst(6000), "60,00 %");
  assert.equal(andelTekst(3333), "33,33 %");
});

test("lesAksjer reads 1 000 as a thousand and refuses fractions and zero", () => {
  assert.deepEqual(lesAksjer("1 000"), { antall: 1000 });
  assert.deepEqual(lesAksjer("1.000"), { antall: 1000 });
  assert.deepEqual(lesAksjer("40"), { antall: 40 });
  for (const bad of ["", "0", "-5", "2,5", "abc"]) assert.ok(lesAksjer(bad).feil, bad);
});

test("grupperTyper splits in from out and marks same-named disposals", () => {
  const typer = [
    { slug: "kjop", navn: "kjøp", tilgang: true, leverbar: true },
    { slug: "ovk", navn: "overføring", tilgang: true, leverbar: true },
    { slug: "ovk_avgang", navn: "overføring", tilgang: false, leverbar: true },
    { slug: "salg", navn: "salg", tilgang: false, leverbar: true },
    { slug: "spleis", navn: "spleis", tilgang: false, leverbar: false },
  ];
  const g = grupperTyper(typer);
  assert.deepEqual(g.tilganger.map((t) => t.slug), ["kjop", "ovk"]);
  assert.deepEqual(g.avganger.map((t) => t.visning), ["overføring (avgang)", "salg", "spleis ⚠"]);
});

test("identTekst shows a birth date, never a number", () => {
  assert.equal(identTekst({ fodselsdato: "1993-02-26" }), "født 26.02.1993");
  assert.equal(identTekst({ orgnr: "923609016" }), "org.nr. 923609016");
});

test("adresseTekst joins what is there", () => {
  assert.equal(adresseTekst({ adresse: "Gata 1", postnummer: "0283", poststed: "OSLO" }), "Gata 1, 0283 OSLO");
  assert.equal(adresseTekst({ landkode: "SE" }), "SE");
});

test("hendelseTekst names direction and counterparty", () => {
  assert.equal(
    hendelseTekst({ type: { navn: "salg", tilgang: false }, antall: 40, holder: "Kari", motpart: "Investor AS", dato: "2026-06-01" }),
    "salg: 40 aksjer for Kari til Investor AS, 01.06.2026.",
  );
  assert.match(hendelseTekst({ type: { navn: "stiftelse", tilgang: true }, antall: 1, holder: "Kari", dato: "2025-01-02" }), /1 aksje for Kari, 02/);
});
