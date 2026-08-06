// UI preferences beyond the theme — the same doctrine as
// theme.svelte.js: per user, localStorage only, never synced through
// the IdP or tokens (docs/portal.md). Currently just the icon style;
// the picker lives in Administrasjon → Utseende.

import { IKONSTILER } from "./ikoner.js";

const KEY = "regnmed-ikonstil";

function stored() {
  try {
    const value = localStorage.getItem(KEY);
    // An unknown stored value (an old build, a typo) falls back to the
    // default rather than rendering nothing.
    return IKONSTILER.some(([slug]) => slug === value) ? value : null;
  } catch (e) {
    return null;
  }
}

export const prefs = $state({ ikonstil: stored() || "linje" });

export function setIkonstil(stil) {
  prefs.ikonstil = stil;
  try {
    localStorage.setItem(KEY, stil);
  } catch (e) {
    /* privat modus */
  }
}
