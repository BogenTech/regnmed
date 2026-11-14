// Unsaved input: a form registers a guard while it holds something not
// yet saved, and leaving the page asks first — by a link anywhere in
// the portal, by Back, or by reload/closing the tab (the browser's own
// warning). One place, so the menu cannot be the way around it.
//
//   onMount(() => vaktUlagret({ ulagret: () => dirty, tittel, melding }))

import { bekreft } from "./dialog.svelte.js";

const vakter = new Set();

export function vaktUlagret(vakt) {
  vakter.add(vakt);
  return () => vakter.delete(vakt);
}

function aktiv() {
  for (const v of vakter) if (v.ulagret()) return v;
  return null;
}

// Resolves true when it is fine to leave: nothing unsaved, or the person
// chose to discard it.
export async function kanForlate() {
  const v = aktiv();
  if (!v) return true;
  return bekreft(v.melding ?? "Det du har skrevet, blir ikke lagret.", {
    tittel: v.tittel ?? "Forlate siden?",
    ok: v.ok ?? "Forlat siden",
    avbryt: v.avbryt ?? "Bli her",
    farlig: true,
  });
}

// After the person chose to leave: the guards belong to a page that is
// going away, and must not ask a second time on the way out.
export function slippVakter() {
  vakter.clear();
}

export function harUlagret() {
  return !!aktiv();
}

window.addEventListener("beforeunload", (e) => {
  if (aktiv()) e.preventDefault();
});

// Links: asked BEFORE the address changes, so nothing has to be undone.
document.addEventListener(
  "click",
  async (e) => {
    const a = e.target.closest?.("a[href^='#']");
    if (!a || e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey) return;
    if (!aktiv()) return;
    e.preventDefault();
    if (await kanForlate()) {
      slippVakter();
      location.hash = a.getAttribute("href");
    }
  },
  true,
);
