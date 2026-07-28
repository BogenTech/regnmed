<script>
  import { logout } from "../lib/auth.svelte.js";
  import { company } from "../lib/me.svelte.js";
  import ThemeControls from "./ThemeControls.svelte";
  import AbonnementBanner from "./AbonnementBanner.svelte";

  import { SEKSJONER, ANSATT_MENY } from "../lib/meny.js";

  let { companyId, section, children } = $props();

  let selskap = $derived(company(companyId));
  let items = $derived(
    selskap?.access === "ansatt"
      ? SEKSJONER.filter(([slug]) => ANSATT_MENY.includes(slug))
      : SEKSJONER,
  );
</script>

<div class="navbar bg-base-100 shadow-sm">
  <div class="flex-1 gap-2 items-baseline">
    <a href="#/" class="btn btn-ghost text-xl">regnmed</a>
    <span class="text-sm opacity-70">{selskap?.name || ""}</span>
  </div>
  <div class="flex-none gap-2">
    <ThemeControls />
    <button class="btn btn-ghost btn-sm" onclick={logout}>Logg ut</button>
  </div>
</div>
<!-- Mobil: menyen legger seg vannrett over innholdet i stedet for å
     spise halve skjermen; fra sm og opp er den sidestilt som før. -->
<div class="flex flex-col sm:flex-row">
  <ul
    class="menu menu-horizontal sm:menu-vertical bg-base-100 w-full sm:w-44 sm:min-h-full flex-nowrap overflow-x-auto sm:overflow-visible"
  >
    {#each items as [slug, label]}
      <li>
        <a href={"#/c/" + companyId + "/" + slug} class={section === slug ? "active" : ""}>
          {label}
        </a>
      </li>
    {/each}
  </ul>
  <main class="flex-1 p-4 sm:p-6 max-w-5xl w-full min-w-0">
    <AbonnementBanner company={selskap} />
    {@render children?.()}
  </main>
</div>
