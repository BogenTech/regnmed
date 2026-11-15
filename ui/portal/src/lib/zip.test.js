import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { crc32, makeZip } from "./zip.js";

test("crc32 matches the standard check value", () => {
  assert.equal(crc32(new TextEncoder().encode("123456789")), 0xcbf43926);
});

test("makeZip writes an archive unzip accepts, byte for byte", (t) => {
  const files = [
    { name: "RF-1086-2026.xml", text: "<a>æøå</a>" },
    { name: "RF-1086-U-2026-1.xml", text: "<b/>" },
  ];
  const dir = mkdtempSync(join(tmpdir(), "zip-test-"));
  const path = join(dir, "t.zip");
  writeFileSync(path, makeZip(files));
  let listing;
  try {
    listing = execFileSync("unzip", ["-Z1", path]).toString();
  } catch {
    t.skip("unzip not installed");
    return;
  }
  assert.deepEqual(listing.trim().split("\n"), files.map((f) => f.name));
  execFileSync("unzip", ["-tq", path]); // throws on a CRC or header error
  assert.equal(execFileSync("unzip", ["-p", path, files[0].name]).toString(), files[0].text);
});
