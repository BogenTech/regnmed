import { test } from "node:test";
import assert from "node:assert/strict";
import { rolleNavn, rolleendringAdvarsel, antallValgt, sammeSett } from "./brukere.js";

test("rolleNavn names the built-in roles and passes custom ones through", () => {
  assert.equal(rolleNavn("bokforing"), "Bokføring");
  assert.equal(rolleNavn("admin"), "Administrator");
  assert.equal(rolleNavn("Fakturerer"), "Fakturerer");
});

test("rolleendringAdvarsel confirms only what can lock someone out", () => {
  const kari = { navn: "Kari", rolle: "admin" };
  const ola = { navn: "Ola", rolle: "ansatt" };
  assert.equal(rolleendringAdvarsel(ola, "les", false), null, "routine change");
  assert.equal(rolleendringAdvarsel(kari, "admin", true), null, "no change");
  assert.match(rolleendringAdvarsel(kari, "les", false), /Kari mister administratortilgangen/);
  assert.match(rolleendringAdvarsel(kari, "les", true), /Du kan ikke ta den tilbake/);
  assert.match(rolleendringAdvarsel(ola, "les", true), /din egen rolle/);
});

test("antallValgt counts the ticked rights in a group", () => {
  const retter = [{ rett: "A" }, { rett: "B" }, { rett: "C" }];
  assert.equal(antallValgt(retter, ["B", "X", "C"]), 2);
});

test("sammeSett ignores order", () => {
  assert.ok(sammeSett(["A", "B"], ["B", "A"]));
  assert.ok(!sammeSett(["A"], ["A", "B"]));
  assert.ok(!sammeSett(["A", "C"], ["A", "B"]));
});
