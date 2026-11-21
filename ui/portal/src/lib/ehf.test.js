import test from "node:test";
import assert from "node:assert/strict";
import { resultMessage, safeLink, dispatchStatus } from "./ehf.js";

test("only sent documents are reported as a success", () => {
  assert.equal(resultMessage({ kanal: "epost", to: "a@b.no" }).ok, true);
  for (const status of ["levert", "akseptert"]) {
    assert.equal(resultMessage({ kanal: "peppol", status, mottaker: "0192:1" }).ok, true);
  }
  for (const status of ["uavklart", "sendeforsok", "leveringsfeil", "avvist", "ikke_sendt"]) {
    assert.equal(resultMessage({ kanal: "peppol", status, mottaker: "0192:1" }).ok, false);
  }
});

test("a taken-over mail job has no address to show", () => {
  assert.equal(resultMessage({ kanal: "epost" }).text, "Sendt på e-post");
});

test("an unresolved outcome does not invite a resend", () => {
  assert.match(resultMessage({ kanal: "peppol", status: "uavklart" }).text, /Ikke send på nytt/);
});

test("a rejection names the rule", () => {
  const message = resultMessage({ kanal: "peppol", status: "avvist", detalj: "BR-CO-16" });
  assert.match(message.text, /BR-CO-16/);
});

test("an unknown status does not look fine", () => {
  assert.equal(dispatchStatus("noe_nytt").tone, "warning");
});

test("only https becomes a link", () => {
  assert.equal(safeLink("https://app.recommand.eu/v"), "https://app.recommand.eu/v");
  assert.equal(safeLink("javascript:alert(1)"), null);
  assert.equal(safeLink("http://x.example"), null);
  assert.equal(safeLink(null), null);
});
