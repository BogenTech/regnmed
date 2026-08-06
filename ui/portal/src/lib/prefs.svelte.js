// The icon style is a PLATFORM setting, locked globally by systemadmin
// (migration 0053, brukerbeslutning 2026-08-06) — no per-user override.
// The portal reads it from /portal-config at boot (App.svelte sets it
// here); the picker lives in the platform console and PUTs
// /platform/settings, then updates this store so the change is visible
// without a reload.

export const prefs = $state({ ikonstil: "linje" });

export function setIkonstil(stil) {
  prefs.ikonstil = stil;
}
