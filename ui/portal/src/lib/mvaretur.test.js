import { test } from "node:test";
import assert from "node:assert/strict";
import { lesRetur, returAdresse } from "./mvaretur.js";

test("the answer from ID-porten names its company and filing", () => {
  const retur = lesRetur("?code=abc&state=company.filing.random");
  assert.deepEqual(retur, {
    companyId: "company",
    filingId: "filing",
    state: "company.filing.random",
    code: "abc",
    error: null,
  });
  assert.equal(returAdresse(retur, "year=2026&termin=1"), "#/c/company/mva?year=2026&termin=1");
  assert.equal(returAdresse(retur, null), "#/c/company/mva");
});

test("a cancelled sign-in is an answer too", () => {
  assert.equal(lesRetur("?error=access_denied&state=c.f.r").error, "access_denied");
});

test("anything else is not a return", () => {
  assert.equal(lesRetur("?code=abc"), null);
  assert.equal(lesRetur("?code=abc&state=only.two"), null);
  assert.equal(lesRetur("?state=c.f.r"), null);
  assert.equal(lesRetur(""), null);
});
