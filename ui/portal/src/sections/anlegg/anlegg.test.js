import { test } from "node:test";
import assert from "node:assert/strict";
import { gevinstTekst, kjoringTekst, lesDriftsmiddel, maned } from "./anlegg.js";

const felt = {
  navn: " Varebil ",
  dato: "2026-03-10",
  kostpris: "450 000",
  restverdi: "",
  levetid: "60",
  balanse: "1238",
  avskr: "6000",
  gruppe: "c",
};

test("lesDriftsmiddel reads amounts the way people type them", () => {
  assert.deepEqual(lesDriftsmiddel(felt).body, {
    navn: "Varebil",
    anskaffelsesdato: "2026-03-10",
    kostpris_ore: 45_000_000,
    restverdi_ore: 0,
    levetid_maneder: 60,
    balansekonto: "1238",
    avskrivningskonto: "6000",
    saldogruppe: "c",
  });
  assert.equal(lesDriftsmiddel({ ...felt, restverdi: "50 000,50" }).body.restverdi_ore, 5_000_050);
});

test("lesDriftsmiddel names what is wrong", () => {
  const feil = (endring) => lesDriftsmiddel({ ...felt, ...endring }).feil;
  assert.match(feil({ navn: " " }), /navn/);
  assert.match(feil({ kostpris: "" }), /Kostprisen/);
  assert.match(feil({ kostpris: "abc" }), /Kostprisen/);
  assert.match(feil({ restverdi: "500 000" }), /lavere/);
  assert.match(feil({ levetid: "0" }), /Levetiden/);
  assert.match(feil({ levetid: "5,5" }), /Levetiden/);
  assert.match(feil({ avskr: "" }), /konto/);
});

test("kjoringTekst counts the postings and lists each failure", () => {
  const svar = {
    generated: 3,
    failed: 1,
    outcomes: [
      { navn: "Varebil", period: "2026-01-01", voucher: "2026-4" },
      { navn: "Henger", period: "2026-05-01", voucher: null, detail: "konto 6010 er deaktivert" },
    ],
  };
  assert.deepEqual(kjoringTekst(svar), {
    bokfort: "3 avskrivninger bokført.",
    feil: ["Henger 05.2026: konto 6010 er deaktivert"],
  });
  assert.match(kjoringTekst({ generated: 0, outcomes: [] }).bokfort, /Ingen/);
  assert.equal(kjoringTekst({ generated: 1, outcomes: [] }).bokfort, "1 avskrivning bokført.");
});

test("maned and gevinstTekst", () => {
  assert.equal(maned("2025-03-01"), "03.2025");
  assert.match(gevinstTekst(100_00), /^Gevinst 100,00/);
  assert.match(gevinstTekst(-100_00), /^Tap 100,00/);
  assert.match(gevinstTekst(0), /Verken/);
});
