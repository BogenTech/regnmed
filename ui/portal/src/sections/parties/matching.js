// Suggested reskontro matches for one party, from its open items (oldest
// first, as the API lists them). The server checks every match; this only
// proposes. Two passes, each item used up as it is assigned:
// 1. exact: a settlement whose open amount equals an invoice's — the
//    payment of that invoice, whatever the dates;
// 2. oldest first: what is left of each settlement against the oldest
//    open invoices — how a payment without a reference is normally read.
// "Invoice side" is the positive remainder (entry_a in the API), the
// "settlement side" the negative one (entry_b); for a supplier the roles
// of invoice and payment are the other way round, which the caller names.
export function suggestPairs(items) {
  const pos = items.filter((i) => i.remaining_ore > 0).map((i) => ({ item: i, left: i.remaining_ore }));
  const neg = items.filter((i) => i.remaining_ore < 0).map((i) => ({ item: i, left: -i.remaining_ore }));
  const pairs = [];
  const take = (p, n, amount, exact) => {
    pairs.push({ a: p.item, b: n.item, amount_ore: amount, exact });
    p.left -= amount;
    n.left -= amount;
  };
  for (const n of neg) {
    const p = pos.find((p) => p.left > 0 && p.left === n.left);
    if (p) take(p, n, n.left, true);
  }
  for (const n of neg) {
    for (const p of pos) {
      if (!n.left) break;
      if (p.left) take(p, n, Math.min(p.left, n.left), false);
    }
  }
  return pairs;
}
