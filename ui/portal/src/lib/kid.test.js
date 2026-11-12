import { test } from "node:test";
import assert from "node:assert/strict";
import { isValidKid } from "./kid.js";

// The cases regnmed-core::kid pins, so the form and the server agree.
test("KID is valid under MOD10 or MOD11, digits only", () => {
  assert.ok(isValidKid("1234566"), "mod10");
  assert.ok(isValidKid("1234560"), "mod11");
  assert.ok(isValidKid("000000018"));
  assert.ok(!isValidKid("1234567"));
  assert.ok(!isValidKid("12345678"));
  assert.ok(!isValidKid("12 34566"));
  assert.ok(!isValidKid("6"));
});
