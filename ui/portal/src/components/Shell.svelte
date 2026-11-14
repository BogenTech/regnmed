<script>
  import { tick } from "svelte";
  import { company } from "../lib/me.svelte.js";
  import { route } from "../lib/router.svelte.js";
  import UserMenu from "./UserMenu.svelte";
  import AbonnementBanner from "./AbonnementBanner.svelte";

  import { SEKSJONER, ANSATT_MENY, FLYTTET_TIL_ADMIN } from "../lib/meny.js";
  import Ikon from "./Ikon.svelte";

  let { companyId, section, children } = $props();

  let selskap = $derived(company(companyId));
  let items = $derived(
    selskap?.access === "ansatt"
      ? SEKSJONER.filter(([slug]) => ANSATT_MENY.includes(slug))
      : SEKSJONER,
  );
  // Old bookmarks to the sections that moved into Administrasjon should
  // still light up the Administrasjon entry.
  let aktiv = $derived(FLYTTET_TIL_ADMIN[section] ? "admin" : section);
  let aktivLabel = $derived(SEKSJONER.find(([slug]) => slug === aktiv)?.[1] || aktiv);

  // Phone: the menu is a list behind a button that names where you are,
  // instead of a strip of 18 sections that hides the current one.
  let menyApen = $state(false);

  // A new page starts at the top, and the keyboard and screen reader
  // land in the content rather than staying on the menu link. Only for a
  // new section or sub-page — a section updating its own ?query (a week,
  // a filter) must not pull focus out of what the person is doing.
  let main = $state(null);
  let forrige = null;
  $effect(() => {
    const seksjon = companyId + "/" + section;
    const side = seksjon + "/" + (route.extra ?? "");
    const ny = forrige !== null && forrige.side !== side;
    const nySeksjon = forrige !== null && forrige.seksjon !== seksjon;
    forrige = { seksjon, side };
    menyApen = false;
    if (!ny) return;
    window.scrollTo(0, 0);
    if (nySeksjon) tick().then(() => main?.focus({ preventScroll: true }));
  });
</script>

<!-- A button, not a #link: the hash is the router's. -->
<button
  class="btn btn-sm btn-primary fixed left-2 top-2 z-50 -translate-y-20 focus:translate-y-0"
  onclick={() => main?.focus()}
>
  Hopp til innhold
</button>
<div class="navbar bg-base-100 shadow-sm">
  <div class="flex-1 gap-2 items-baseline min-w-0">
    <a href="#/" class="btn btn-ghost text-xl">regnmed</a>
    <span class="text-sm opacity-70 truncate">{selskap?.name || ""}</span>
  </div>
  <div class="flex-none gap-2">
    <UserMenu innstillingerHref={"#/c/" + companyId + "/admin"} />
  </div>
</div>
<div class="flex flex-col sm:flex-row">
  <nav aria-label="Seksjoner" class="bg-base-100 sm:w-48 sm:min-h-full">
    <button
      class="btn btn-ghost w-full justify-between sm:hidden rounded-none border-b border-base-200"
      aria-expanded={menyApen}
      aria-controls="seksjonsmeny"
      onclick={() => (menyApen = !menyApen)}
    >
      <span class="flex items-center gap-2"><Ikon navn={aktiv} /> {aktivLabel}</span>
      <span>Meny <span aria-hidden="true">{menyApen ? "▴" : "▾"}</span></span>
    </button>
    <ul id="seksjonsmeny" class="menu w-full {menyApen ? '' : 'hidden'} sm:flex">
      {#each items as [slug, label]}
        <li>
          <a
            href={"#/c/" + companyId + "/" + slug}
            class="flex items-center gap-2 {aktiv === slug
              ? 'bg-base-200 text-primary font-semibold shadow-[inset_3px_0_0_0_currentColor]'
              : ''}"
            aria-current={aktiv === slug ? "page" : undefined}
          >
            <Ikon navn={slug} />
            {label}
          </a>
        </li>
      {/each}
    </ul>
  </nav>
  <main
    id="innhold"
    bind:this={main}
    tabindex="-1"
    class="flex-1 p-4 sm:p-6 max-w-5xl w-full min-w-0 outline-none"
  >
    <!-- Where am I: selskapsliste → selskap → seksjon. Depth within a
         section (faner, undersider) is the section's own tab bar. -->
    <div class="breadcrumbs text-sm pt-0 pb-3">
      <ul>
        <li><a href="#/">Mine selskaper</a></li>
        <li><a href={"#/c/" + companyId + "/oversikt"}>{selskap?.name || "…"}</a></li>
        <li>{aktivLabel}</li>
      </ul>
    </div>
    {#if selskap}
      <AbonnementBanner company={selskap} />
      {@render children?.()}
    {:else}
      <!-- Not among the companies /me says this person may act for: a
           wrong address, or access that has ended. -->
      <div role="alert" class="alert alert-warning">
        <div>
          <p>Fant ikke selskapet. Adressen kan være feil, eller tilgangen din er avsluttet.</p>
          <a class="link" href="#/">Til Mine selskaper</a>
        </div>
      </div>
    {/if}
  </main>
</div>
