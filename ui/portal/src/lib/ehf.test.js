import test from "node:test";
import assert from "node:assert/strict";
import { createRouteCache, resultMessage, safeLink, dispatchStatus } from "./ehf.js";

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

test("a started channel check is shared until it expires", async () => {
  let time = 0;
  let loads = 0;
  const cache = createRouteCache({ ttlMs: 100, now: () => time });
  const load = async () => ++loads;
  assert.equal(await cache.get("a", load), 1);
  assert.equal(await cache.get("a", load), 1);
  time = 100;
  assert.equal(await cache.get("a", load), 2);
  cache.clear("a");
  assert.equal(await cache.get("a", load), 3);
});

test("storing a route removes expired ones", async () => {
  let time = 0;
  let loads = 0;
  const cache = createRouteCache({ ttlMs: 100, now: () => time });
  const load = async () => ++loads;
  await cache.get("old", load);
  time = 150;
  await cache.get("new", load);
  time = 160;
  assert.equal(await cache.get("new", load), 2);
  assert.equal(await cache.get("old", load), 3);
});

test("a failed channel check is not kept", async () => {
  let loads = 0;
  const cache = createRouteCache();
  const fail = async () => {
    loads++;
    throw new Error("down");
  };
  await assert.rejects(cache.get("a", fail));
  await assert.rejects(cache.get("a", fail));
  assert.equal(loads, 2);
});
