import test from "node:test";
import assert from "node:assert/strict";
import { dagerIgjen, koblingsstatus, maskert, retur } from "./bankkobling.js";

const idag = "2026-11-19";

test("dagerIgjen counts whole days and tolerates missing dates", () => {
  assert.equal(dagerIgjen("2026-11-19", idag), 0);
  assert.equal(dagerIgjen("2026-12-03", idag), 14);
  assert.equal(dagerIgjen("2026-11-18", idag), -1);
  assert.equal(dagerIgjen(null, idag), null);
  assert.equal(dagerIgjen("snart", idag), null);
});

test("status nags from 14 days out and when expired, by date or by the server's word", () => {
  const s = (k) => koblingsstatus(k, idag);
  assert.deepEqual(s({ status: "aktiv", gyldig_til: "2027-03-01" }), { tekst: "aktiv", klasse: "badge-success", forny: false });
  assert.equal(s({ status: "aktiv", gyldig_til: "2026-12-04" }).forny, false, "15 days left");
  assert.equal(s({ status: "aktiv", gyldig_til: "2026-12-03" }).tekst, "utløper om 14 dager");
  assert.equal(s({ status: "aktiv", gyldig_til: "2026-11-20" }).tekst, "utløper om 1 dag");
  assert.equal(s({ status: "aktiv", gyldig_til: "2026-11-19" }).tekst, "utløper i dag");
  assert.equal(s({ status: "aktiv", gyldig_til: "2026-11-18" }).tekst, "samtykket er utløpt");
  assert.equal(s({ status: "utlopt", gyldig_til: "2027-01-01" }).forny, true, "the bank ended it early");
  assert.equal(s({ status: "venter", gyldig_til: null }).forny, true);
  assert.equal(s({ status: "avsluttet", gyldig_til: "2026-11-18" }).forny, false);
});

test("maskert keeps the last five digits only", () => {
  assert.equal(maskert("1234 56 78903"), "•••• •• 78903");
  assert.equal(maskert("12345678903"), "•••• •• 78903");
  assert.equal(maskert(null), "");
});

test("retur reads the bank's return parameters", () => {
  assert.deepEqual(retur("bankkode=abc&banktilstand=s1"), { kode: "abc", tilstand: "s1" });
  assert.deepEqual(retur("bankfeil=Avbrutt%20av%20bruker"), { feil: "Avbrutt av bruker" });
  assert.equal(retur("bankkode=abc"), null, "a code without its state is not a return");
  assert.equal(retur("year=2026"), null);
  assert.equal(retur(""), null);
});
