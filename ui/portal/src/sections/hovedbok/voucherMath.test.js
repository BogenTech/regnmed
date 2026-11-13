import { test } from "node:test";
import assert from "node:assert/strict";
import { lineOre, postingLines } from "./voucherMath.js";

test("debit is positive, credit negative, one column per line", () => {
  assert.equal(lineOre({ debit: "1 250,00" }), 125_000);
  assert.equal(lineOre({ credit: "1 250" }), -125_000);
  assert.equal(lineOre({}), null);
  assert.ok(Number.isNaN(lineOre({ debit: "1", credit: "1" })));
  assert.ok(Number.isNaN(lineOre({ debit: "-5" })));
});

test("a VAT code splits the gross amount, as the core does", () => {
  const lines = postingLines([
    { account: "6800", debit: "1 250", vat: "1" },
    { account: "1920", credit: "1 250" },
  ]);
  assert.deepEqual(
    lines.map((l) => [l.account, l.amount_ore, l.vat_code]),
    [["6800", 100_000, "1"], ["2710", 25_000, null], ["1920", -125_000, null]],
  );
  const sale = postingLines([
    { account: "1920", debit: "100" },
    { account: "3000", credit: "100", vat: "3" },
  ]);
  assert.deepEqual(sale.map((l) => [l.account, l.amount_ore]), [["1920", 10_000], ["3000", -8_000], ["2700", -2_000]]);
  assert.equal(sale.reduce((s, l) => s + l.amount_ore, 0), 0);
});
