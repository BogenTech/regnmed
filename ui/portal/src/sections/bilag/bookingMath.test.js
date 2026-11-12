import { test } from "node:test";
import assert from "node:assert/strict";
import { splitGross, simpleLines, guessVatCode } from "./bookingMath.js";

test("splitGross mirrors regnmed-core: base + vat == gross, half away from zero", () => {
  assert.deepEqual(splitGross(125000, 2500), { base: 100000, vat: 25000 });
  assert.deepEqual(splitGross(100, 2500), { base: 80, vat: 20 });
  assert.deepEqual(splitGross(3, 2500), { base: 2, vat: 1 }); // 2,4 -> 2
  assert.deepEqual(splitGross(-125000, 2500), { base: -100000, vat: -25000 });
  assert.deepEqual(splitGross(4999, 0), { base: 4999, vat: 0 });
});

test("a supplier invoice posts cost, input VAT and the debt to the supplier, balanced", () => {
  const lines = simpleLines({
    grossOre: 125000, vatCode: "1", costAccount: "6300", payment: "faktura", partyNo: "50000",
  });
  assert.deepEqual(lines.map((l) => [l.account, l.amount_ore, l.party_no]), [
    ["6300", 100000, null],
    ["2710", 25000, null],
    ["2400", -125000, "50000"],
  ]);
  assert.equal(lines.reduce((s, l) => s + l.amount_ore, 0), 0);
});

test("a receipt without VAT paid from the bank is two lines, no party", () => {
  const lines = simpleLines({ grossOre: 4990, vatCode: "", costAccount: "7140", payment: "bank" });
  assert.deepEqual(lines.map((l) => [l.account, l.amount_ore, l.party_no]), [
    ["7140", 4990, null],
    ["1920", -4990, null],
  ]);
});

test("the VAT code is guessed from the suggestion's amounts", () => {
  assert.equal(guessVatCode(100000, 25000), "1");
  assert.equal(guessVatCode(100000, 15000), "11");
  assert.equal(guessVatCode(100000, 12000), "13");
  assert.equal(guessVatCode(100000, 0), "");
});
