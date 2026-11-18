import { test } from "node:test";
import assert from "node:assert/strict";
import { renderMarkdown } from "./markdown.js";

test("the common agent markdown renders", () => {
  assert.equal(
    renderMarkdown(
      "Selskapet har ett prosjekt: **Testprosjekt** (kode 10001).",
    ),
    "<p>Selskapet har ett prosjekt: <strong>Testprosjekt</strong> (kode 10001).</p>",
  );
  assert.equal(
    renderMarkdown("*viktig* og `valid_from`"),
    "<p><em>viktig</em> og <code>valid_from</code></p>",
  );
  assert.equal(
    renderMarkdown(
      "Satsene:\n- 1 500 kr fra 2026-08-07\n- 1 650 kr fra 2026-12-01\n\nVelg dato.",
    ),
    "<p>Satsene:</p><ul><li>1 500 kr fra 2026-08-07</li><li>1 650 kr fra 2026-12-01</li></ul><p>Velg dato.</p>",
  );
  assert.equal(
    renderMarkdown("1. Les\n2. Skriv"),
    "<ol><li>Les</li><li>Skriv</li></ol>",
  );
  assert.equal(
    renderMarkdown("## Oppsummering\nlinje en\nlinje to"),
    "<p><strong>Oppsummering</strong></p><p>linje en<br>linje to</p>",
  );
});

test("nothing a model writes becomes markup of its own", () => {
  const out = renderMarkdown(
    '<img src=x onerror=alert(1)> [lenke](javascript:alert(1)) "sitat"',
  );
  assert.ok(!out.includes("<img"), out);
  assert.ok(!out.includes("<a"), out);
  assert.ok(out.includes("&lt;img"), out);
  assert.ok(out.includes("&quot;sitat&quot;"), out);
});

test("ordinary text is left alone", () => {
  assert.equal(
    renderMarkdown("5 * 3 = 15, a_b_c og 2*x"),
    "<p>5 * 3 = 15, a_b_c og 2*x</p>",
  );
  assert.equal(
    renderMarkdown("**`kode`**"),
    "<p><strong><code>kode</code></strong></p>",
  );
});
