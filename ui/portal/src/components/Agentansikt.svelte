<script>
  // An agent's face (docs/agent.md §8.1): initials, a colour and
  // optionally an icon from the portal's own set. No uploaded image —
  // humans here have initials too, because identity data belongs to
  // regnid and is only mirrored (#68).
  import Ikon from "./Ikon.svelte";
  import { IKONER } from "../lib/ikoner.js";

  // Tailwind v4 emits only classes it finds in the source, so every
  // class is spelled out here. The server validates the colour name
  // against its own list (regnmed_db::AVATAR_FARGER); this table is the
  // renderer, and an unknown name falls back rather than disappearing.
  const FARGEKLASSER = {
    primary: "bg-primary text-primary-content",
    secondary: "bg-secondary text-secondary-content",
    accent: "bg-accent text-accent-content",
    info: "bg-info text-info-content",
    success: "bg-success text-success-content",
    warning: "bg-warning text-warning-content",
    error: "bg-error text-error-content",
    neutral: "bg-neutral text-neutral-content",
  };

  let { navn, farge = null, ikon = null, storrelse = "w-9" } = $props();

  const klasse = $derived(FARGEKLASSER[farge] || FARGEKLASSER.neutral);
  // Ikon renders nothing for a name it does not know, which would leave
  // an empty circle — so an unknown name falls back to the initials.
  const visIkon = $derived(ikon && IKONER[ikon] ? ikon : null);
  const initialer = $derived(
    (navn || "?")
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((del) => del[0].toUpperCase())
      .join("") || "?",
  );
</script>

<div class="avatar avatar-placeholder shrink-0" title={navn}>
  <div class="{klasse} {storrelse} rounded-full">
    {#if visIkon}
      <Ikon navn={visIkon} />
    {:else}
      <span class="text-xs font-semibold">{initialer}</span>
    {/if}
  </div>
</div>
