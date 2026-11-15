<script>
  // Produktregisteret (#39): nummeret er permanent, og produkter
  // deaktiveres — de slettes aldri, fordi utstedte dokumenter har
  // kopiert verdiene sine.
  //
  // Stacked rows rather than a seven-column table so it fits a phone;
  // one row opens for editing at a time, in place.
  import { tick } from "svelte";
  import { kr } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import Produktskjema from "./Produktskjema.svelte";
  import { mvaTekst, utvalg } from "./produkt.js";

  let { companyId, products, kanSkrive, kontoer, onChanged } = $props();

  let sok = $state("");
  let visDeaktiverte = $state(false);
  let apen = $state(null);
  let del = $derived(utvalg(products, sok));

  async function lukk(nummer) {
    apen = null;
    await tick();
    // A product just deactivated has moved behind the toggle.
    (document.getElementById("produkt-" + nummer) ?? document.getElementById("deaktiverte"))?.focus();
  }

  const detaljer = (p) =>
    [kr(p.salgspris_ore) + " kr", mvaTekst(p.vat_code), "konto " + p.konto, p.lagerfort && "lagerført"]
      .filter(Boolean)
      .join(", ");
</script>

{#snippet rader(liste)}
  <ul class="divide-y divide-base-200">
    {#each liste as p (p.nummer)}
      <li class="py-2">
        <div class="flex items-center gap-2">
          <div class="min-w-0 flex-1">
            <div class="truncate">
              <span class="font-medium">{p.nummer}</span>
              {p.navn}
              {#if !p.aktiv}<span class="badge badge-xs badge-ghost ml-1">deaktivert</span>{/if}
            </div>
            <div class="text-xs opacity-60">{detaljer(p)}</div>
          </div>
          {#if kanSkrive}
            <button
              id={"produkt-" + p.nummer}
              class="btn btn-xs btn-ghost shrink-0"
              aria-expanded={apen === p.nummer}
              aria-label={(apen === p.nummer ? "Lukk " : "Endre ") + p.nummer + " " + p.navn}
              onclick={() => (apen === p.nummer ? lukk(p.nummer) : (apen = p.nummer))}
            >
              {apen === p.nummer ? "Lukk" : "Endre"}
            </button>
          {/if}
        </div>
        {#if apen === p.nummer}
          <Produktskjema
            {companyId}
            produkt={p}
            {kontoer}
            onSaved={onChanged}
            onClose={() => lukk(p.nummer)}
          />
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

<Card title="Produktregister">
  <p class="text-sm opacity-70 mb-2">
    Velg et produkt på fakturaen, så fylles navn, pris, mva og konto ut. Fakturaen tar en kopi, så
    endringer her rører aldri det som alt er utstedt. Nummeret kan ikke endres, og produkter
    deaktiveres i stedet for å slettes.
  </p>
  {#if products.length > 8}
    <label class="input input-sm mb-2 w-full sm:w-72">
      <span class="sr-only">Søk i produkter</span>
      <input type="search" placeholder="Søk på nummer eller navn" bind:value={sok} />
    </label>
  {/if}
  {#if del.aktive.length}
    {@render rader(del.aktive)}
  {:else}
    <p class="text-sm opacity-70">
      {sok.trim() ? "Ingen aktive produkter passer søket." : "Ingen aktive produkter ennå."}
    </p>
  {/if}
  {#if del.deaktiverte.length}
    <button
      id="deaktiverte"
      class="btn btn-xs btn-ghost mt-1"
      aria-expanded={visDeaktiverte}
      onclick={() => (visDeaktiverte = !visDeaktiverte)}
    >
      {visDeaktiverte ? "Skjul" : "Vis"} deaktiverte ({del.deaktiverte.length})
    </button>
    {#if visDeaktiverte}
      {@render rader(del.deaktiverte)}
    {/if}
  {/if}
  {#if kanSkrive}
    <h3 class="font-semibold mt-4">Nytt produkt</h3>
    <Produktskjema {companyId} {kontoer} onSaved={onChanged} />
  {/if}
</Card>
