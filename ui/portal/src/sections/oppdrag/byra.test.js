import { test } from "node:test";
import assert from "node:assert/strict";
import { finnByraer, formatOrgnr } from "./byra.js";

const firms = [
  { name: "Tall og Tre AS", orgnr: "912345678", kind: "regnskap" },
  { name: "Revisjonshuset AS", orgnr: "998877665", kind: "revisjon" },
  { name: "Tallknuseriet AS", orgnr: "911222333", kind: "regnskap" },
];

test("an empty search lists every firm of the chosen kind", () => {
  assert.equal(finnByraer(firms, "").length, 3);
  assert.deepEqual(
    finnByraer(firms, "", "revisjon").map((f) => f.name),
    ["Revisjonshuset AS"],
  );
});

test("name matches any part, any case", () => {
  assert.deepEqual(
    finnByraer(firms, "  TALL ").map((f) => f.name),
    ["Tall og Tre AS", "Tallknuseriet AS"],
  );
});

test("orgnr matches with or without spaces, and letters never match digits", () => {
  assert.deepEqual(
    finnByraer(firms, "912 345").map((f) => f.name),
    ["Tall og Tre AS"],
  );
  assert.equal(finnByraer(firms, "91a").length, 0);
});

test("orgnr is grouped as Brønnøysund prints it", () => {
  assert.equal(formatOrgnr("912345678"), "912 345 678");
  assert.equal(formatOrgnr("12"), "12");
});
