<script>
  import { company } from "../lib/me.svelte.js";
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
</script>

<div class="navbar bg-base-100 shadow-sm">
  <div class="flex-1 gap-2 items-baseline">
    <a href="#/" class="btn btn-ghost text-xl">regnmed</a>
    <span class="text-sm opacity-70">{selskap?.name || ""}</span>
  </div>
  <div class="flex-none gap-2">
    <UserMenu innstillingerHref={"#/c/" + companyId + "/admin"} />
  </div>
</div>
<!-- Mobil: menyen legger seg vannrett over innholdet i stedet for å
     spise halve skjermen; fra sm og opp er den sidestilt som før. -->
<div class="flex flex-col sm:flex-row">
  <ul
    class="menu menu-horizontal sm:menu-vertical bg-base-100 w-full sm:w-48 sm:min-h-full flex-nowrap overflow-x-auto sm:overflow-visible"
  >
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
  <main class="flex-1 p-4 sm:p-6 max-w-5xl w-full min-w-0">
    <!-- Where am I: selskapsliste → selskap → seksjon. Depth within a
         section (faner, undersider) is the section's own tab bar. -->
    <div class="breadcrumbs text-sm pt-0 pb-3">
      <ul>
        <li><a href="#/">Mine selskaper</a></li>
        <li><a href={"#/c/" + companyId + "/oversikt"}>{selskap?.name || "…"}</a></li>
        <li>{aktivLabel}</li>
      </ul>
    </div>
    <AbonnementBanner company={selskap} />
    {@render children?.()}
  </main>
</div>
