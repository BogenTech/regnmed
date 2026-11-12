import { test } from "node:test";
import assert from "node:assert/strict";
import { lineNetOre, vatOfBase, milliHours } from "./invoiceMath.js";

// The same cases regnmed-core rounds; a drift here shows the user a sum
// the issued invoice will not have.
test("line amounts round half away from zero like regnmed-core", () => {
  assert.equal(lineNetOre(2500, 400000), 1000000); // 2,5 × 4 000 kr
  assert.equal(lineNetOre(1500, 1), 2); // 1,5 øre -> 2
  assert.equal(lineNetOre(-1500, 1), -2);
  assert.equal(lineNetOre(333, 100), 33); // 33,3 -> 33
});

test("vat rounds half away from zero in basis points", () => {
  assert.equal(vatOfBase(1000000, 2500), 250000);
  assert.equal(vatOfBase(2, 2500), 1); // 0,5 -> 1
  assert.equal(vatOfBase(-2, 2500), -1);
  assert.equal(vatOfBase(1, 1500), 0); // 0,15 -> 0
});

test("minutes become thousandths of an hour with the server's rounding", () => {
  assert.equal(milliHours(60), 1000);
  assert.equal(milliHours(1), 17); // 16,67 -> 17
  assert.equal(milliHours(450), 7500);
});
