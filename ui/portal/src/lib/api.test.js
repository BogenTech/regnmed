// The GET cache in api.js (`npm test`, run in CI). api.js imports the
// rune-based auth module, which plain Node cannot load, so the test loads
// a copy of api.js with that one import replaced by stubs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile, writeFile, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const source = await readFile(new URL("./api.js", import.meta.url), "utf8");
const stubbed = source.replace(
  /^import \{ tokens, clearTokens, refresh \} from "\.\/auth\.svelte\.js";$/m,
  'const tokens = () => ({ access_token: "t" }), clearTokens = () => {}, refresh = async () => false;',
);
assert.notEqual(stubbed, source, "the auth import moved; update this test");
const file = join(await mkdtemp(join(tmpdir(), "api-test-")), "api.mjs");
await writeFile(file, stubbed);

globalThis.document = { addEventListener() {}, visibilityState: "visible" };
let calls = 0;
let value = 1;
let gate = null;
globalThis.fetch = async (_path, options) => {
  calls++;
  if (gate && (options.method || "GET") === "GET") await gate;
  return new Response(JSON.stringify({ v: value }), {
    headers: { "content-type": "application/json" },
  });
};
const { api } = await import(file);

test("a repeated read is served from the cache, as a fresh copy", async () => {
  calls = 0;
  assert.deepEqual(await api("/a"), { v: 1 });
  const copy = await api("/a");
  assert.equal(calls, 1);
  copy.v = 99;
  assert.deepEqual(await api("/a"), { v: 1 });
});

test("concurrent identical reads share one request", async () => {
  calls = 0;
  await Promise.all([api("/b"), api("/b")]);
  assert.equal(calls, 1);
});

test("any write empties the cache", async () => {
  await api("/c");
  value = 2;
  await api("/anything", { method: "PUT" });
  assert.deepEqual(await api("/c"), { v: 2 });
});

test("a read that started before a write is not cached after it", async () => {
  let open;
  gate = new Promise((resolve) => (open = resolve));
  const slow = api("/d"); // reads the value from before the write
  gate = null;
  value = 3;
  await api("/write", { method: "POST" });
  open();
  await slow;
  calls = 0;
  assert.deepEqual(await api("/d"), { v: 3 });
  assert.equal(calls, 1);
});

test("/me is never cached: it redeems invitations", async () => {
  calls = 0;
  await api("/me");
  await api("/me");
  assert.equal(calls, 2);
});
