import test from "node:test";
import assert from "node:assert/strict";
import { parseTider, planTekst, sistTekst, tiderTekst } from "./tidsplan.js";

test("parseTider normalises, sorts and removes duplicates", () => {
  assert.deepEqual(parseTider("12:00, 3:15"), ["03:15", "12:00"]);
  assert.deepEqual(parseTider("03.15 03:15;18:00"), ["03:15", "18:00"]);
  assert.deepEqual(parseTider(" 23:59 "), ["23:59"]);
});

test("parseTider refuses what is not a time, nothing at all, and too many", () => {
  for (const bad of ["", "  ", "24:00", "12:60", "3", "0315", "tre", "03:15, x"]) {
    assert.throws(() => parseTider(bad), undefined, bad);
  }
  assert.throws(() => parseTider("01:00, 02:00, 03:00", 2), /høyst 2/);
  assert.deepEqual(parseTider("01:00, 01:00, 02:00", 2), ["01:00", "02:00"], "duplicates do not count");
});

test("times read naturally", () => {
  assert.equal(tiderTekst([]), "");
  assert.equal(tiderTekst(["03:15"]), "03:15");
  assert.equal(tiderTekst(["03:15", "12:00"]), "03:15 og 12:00");
  assert.equal(tiderTekst(["03:15", "12:00", "18:00"]), "03:15, 12:00 og 18:00");
});

test("the schedule line says when, or that it is paused", () => {
  assert.equal(planTekst({ aktiv: true, tider: ["03:15"] }), "Henter automatisk hver dag kl. 03:15");
  assert.equal(planTekst({ aktiv: false, tider: ["03:15"] }), "Automatisk henting er satt på pause");
});

test("the last run's outcome: counts, the first error, a failure, in progress, none", () => {
  assert.equal(sistTekst(null, "x"), null);
  assert.deepEqual(sistTekst({ status: "running" }, "x"), { tekst: "Henter nå …", feil: false });
  assert.deepEqual(sistTekst({ status: "done", resultat: { nye: 3, feil: [] } }, "i natt"), {
    tekst: "Sist i natt: 3 nye bankposter",
    feil: false,
  });
  assert.equal(sistTekst({ status: "done", resultat: { nye: 1 } }, "").tekst, "Sist: 1 ny bankpost");
  assert.equal(sistTekst({ status: "done", resultat: {} }, "").tekst, "Sist: ingen nye bankposter");
  assert.deepEqual(sistTekst({ status: "done", resultat: { nye: 2, feil: ["Samtykket er utløpt.", "x"] } }, ""), {
    tekst: "Sist: Samtykket er utløpt.",
    feil: true,
  });
  assert.deepEqual(sistTekst({ status: "failed", feil: "ukjent jobb" }, "19.11"), {
    tekst: "Sist 19.11: feilet: ukjent jobb",
    feil: true,
  });
});
