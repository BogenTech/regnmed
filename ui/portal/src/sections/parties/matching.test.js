import { test } from "node:test";
import assert from "node:assert/strict";
import { suggestPairs } from "./matching.js";

const item = (id, ore) => ({ entry_id: id, remaining_ore: ore });
const brief = (pairs) => pairs.map((p) => [p.a.entry_id, p.b.entry_id, p.amount_ore, p.exact]);

test("an exact amount pairs with its payment, whatever the order", () => {
  const items = [item("f1", 12_500_00), item("f2", 1_250_00), item("p1", -1_250_00)];
  assert.deepEqual(brief(suggestPairs(items)), [["f2", "p1", 1_250_00, true]]);
});

test("a payment without its exact invoice goes to the oldest first", () => {
  const items = [item("f1", 3_000_00), item("f2", 4_000_00), item("p1", -5_000_00)];
  assert.deepEqual(brief(suggestPairs(items)), [
    ["f1", "p1", 3_000_00, false],
    ["f2", "p1", 2_000_00, false],
  ]);
});

test("nothing is suggested twice, and one-sided items give nothing", () => {
  const items = [item("f1", 1_000_00), item("p1", -1_000_00), item("p2", -1_000_00)];
  assert.deepEqual(brief(suggestPairs(items)), [["f1", "p1", 1_000_00, true]]);
  assert.deepEqual(suggestPairs([item("f1", 5)]), []);
});
