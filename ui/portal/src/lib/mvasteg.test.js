import { test } from "node:test";
import assert from "node:assert/strict";
import { lukkeTekst, nesteSteg, stoppTekst } from "./mvasteg.js";

const filing = (status, flagg = {}) => ({
  status,
  jobb_aktiv: false,
  henter_svar: false,
  kan_godkjennes: true,
  ...flagg,
});

test("a running job is waited for, and offers nothing", () => {
  const steg = nesteSteg(filing("godkjent", { jobb_aktiv: true, kan_godkjennes: false }));
  assert.deepEqual(steg, { handling: null, poll: 2_000, stoppet: false });
});

test("a job that died stops the polling and offers the way on", () => {
  for (const status of ["godkjent", "oppretter_instans", "instans_opprettet"]) {
    assert.deepEqual(nesteSteg(filing(status)), { handling: "fortsett", poll: null, stoppet: true });
  }
});

test("a sent filing waits only while the worker can fetch the answer", () => {
  assert.equal(nesteSteg(filing("sendt", { henter_svar: true })).poll, 15_000);
  assert.equal(nesteSteg(filing("sendt", { henter_svar: true })).handling, null);
  assert.deepEqual(nesteSteg(filing("sendt")), { handling: "hent", poll: null, stoppet: false });
});

test("nothing is offered where the server says no", () => {
  const steg = nesteSteg(filing("uavklart", { kan_godkjennes: false }));
  assert.equal(steg.handling, null);
  assert.equal(steg.poll, null);
  assert.equal(nesteSteg(filing("kvittert", { kan_godkjennes: false })).handling, null);
});

test("a closed attempt or no filing starts fresh", () => {
  assert.equal(nesteSteg(null).handling, "godkjenn");
  for (const status of ["avvik", "avvist", "avbrutt"]) {
    assert.equal(nesteSteg(filing(status, { kan_godkjennes: false })).handling, "godkjenn");
  }
  const ikkeLevert = { funn: "ikke_levert" };
  assert.equal(
    nesteSteg(filing("lukket_etter_kontroll", { kan_godkjennes: false, lukking: ikkeLevert })).handling,
    "godkjenn",
  );
  assert.equal(nesteSteg(filing("opprettet")).handling, "godkjenn");
});

test("a stopped run with an instance never suggests nothing was sent", () => {
  const tekst = stoppTekst(filing("instans_opprettet", { kan_vaere_sendt: true }));
  assert.match(tekst, /Sjekk Altinn før du gjør noe annet/);
  assert.doesNotMatch(tekst, /fortsetter vi/);
  assert.match(stoppTekst(filing("godkjent", { kan_vaere_sendt: false })), /fortsetter vi/);
});

test("a term a person found delivered is never offered again", () => {
  const lukking = { funn: "levert" };
  const steg = nesteSteg(filing("lukket_etter_kontroll", { kan_godkjennes: false, lukking }));
  assert.deepEqual(steg, { handling: null, poll: null, stoppet: false });
  assert.match(lukkeTekst(lukking), /ifølge kontroll i Altinn/);
  assert.match(lukkeTekst(lukking), /ikke hentet kvittering/);
  assert.match(lukkeTekst({ funn: "ikke_levert" }), /Ikke levert/);
});
