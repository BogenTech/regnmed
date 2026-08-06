<script>
  // One section icon, rendered in the user's chosen style. The markup
  // in ikoner.js is checked-in and static — {@html} never sees user
  // input here. Icons are decoration next to a visible label, so they
  // are aria-hidden throughout.
  import { IKONER } from "../lib/ikoner.js";
  import { prefs } from "../lib/prefs.svelte.js";

  let { navn, stil = null } = $props();

  const valgt = $derived(stil || prefs.ikonstil);
  const ikon = $derived(IKONER[navn]);
</script>

{#if ikon && valgt === "emoji"}
  <span class="text-base leading-none shrink-0" aria-hidden="true">{ikon.emoji}</span>
{:else if ikon && (valgt === "linje" || valgt === "kraftig")}
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 24 24"
    class="w-[1.15em] h-[1.15em] shrink-0"
    fill="none"
    stroke="currentColor"
    stroke-width={valgt === "kraftig" ? 2.4 : 1.6}
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    {@html ikon.d}
  </svg>
{/if}
