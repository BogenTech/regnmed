import { test } from "node:test";
import assert from "node:assert/strict";
import { KILDE, gjoremal } from "./admin.js";

const ferdig = {
  address: "Storgata 1",
  bank_account: "1234.56.78903",
  email: "post@firma.no",
  registrering_historikk: [{ kilde: "brreg" }],
};

test("gjoremal is empty for a company that is ready", () => {
  assert.deepEqual(gjoremal({ settings: ferdig, terms: { godtatt: true }, abo: { status: "aktiv" } }), []);
});

test("gjoremal names each missing piece and where it is fixed", () => {
  const ut = gjoremal({
    settings: { address: " ", bank_account: "", email: null, registrering_historikk: [{ kilde: "migrert" }] },
    terms: { godtatt: false },
    abo: { status: "frist" },
  });
  assert.equal(ut.length, 6);
  assert.match(ut[0].tekst, /adressen/);
  assert.match(ut[1].tekst, /kontonummeret/);
  assert.match(ut[2].tekst, /e-postadressen/);
  assert.match(ut[3].tekst, /Bekreft/);
  assert.match(ut[4].tekst, /kundevilkårene/);
  assert.equal(ut[5].fane, "abonnement");
});

test("gjoremal raises nothing for facts it could not read", () => {
  assert.deepEqual(gjoremal({ settings: null, terms: null, abo: null }), []);
});

test("KILDE names every stored source", () => {
  for (const k of ["brreg", "manuell", "migrert"]) assert.ok(KILDE[k]);
});
