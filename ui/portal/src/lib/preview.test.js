import { test } from "node:test";
import assert from "node:assert/strict";
import { sniffType } from "./preview.js";

const bytes = (s) => new Uint8Array([...s].map((c) => c.charCodeAt(0)));

test("the bytes decide, not the name", () => {
  assert.equal(sniffType(bytes("%PDF-1.7")), "application/pdf");
  assert.equal(sniffType(new Uint8Array([0xff, 0xd8, 0xff, 0xe0])), "image/jpeg");
  assert.equal(sniffType(new Uint8Array([0x89, 0x50, 0x4e, 0x47])), "image/png");
  assert.equal(sniffType(bytes("RIFF....WEBP")), "image/webp");
  assert.equal(sniffType(bytes("RIFF....WAVE")), null);
  assert.equal(sniffType(bytes("<html><script>")), null);
});
