<script>
  // Kundesiden: hva kunden skylder, kundens fakturaer og kontaktinfoen
  // fakturaene sendes med. Postene i reskontroen (og manuell matching)
  // ligger under, for regnskapsføreren.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";
  import PartyContact from "../../components/PartyContact.svelte";
  import InvoiceList from "../faktura/InvoiceList.svelte";
  import SubledgerItems from "../parties/SubledgerItems.svelte";

  let { companyId, partyId, query = "" } = $props();
  // ?avstem (from Reskontro → Oversikt) opens the reskontro straight away.
  const startOpen = new URLSearchParams(query).has("avstem");
  // Arriving to match: bring the reskontro into view once the party is shown.
  let scrolled = false;
  $effect(() => {
    if (!startOpen || !party || scrolled) return;
    scrolled = true;
    queueMicrotask(() => document.getElementById("reskontro")?.scrollIntoView());
  });

  let party = $state(null);
  let error = $state(null);

  function load() {
    api("/companies/" + companyId + "/parties/" + partyId)
      .then((p) => (party = p))
      .catch((e) => (error = e.message));
  }
  $effect(load);

  let canEditContact = $derived(harRett(companyId, "KONTAKT_SKRIV"));
  let canInvoice = $derived(harRett(companyId, "FAKTURA_SKRIV"));
</script>

<nav class="mb-3">
  <a class="link link-hover text-sm" href={"#/c/" + companyId + "/kunder"}>← Alle kunder</a>
</nav>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if !party}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else if party.kind !== "kunde"}
  <div class="alert alert-info">
    <span>{party.name} er en leverandør, ikke en kunde.</span>
    <a class="btn btn-sm" href={"#/c/" + companyId + "/leverandorer/" + party.party_id}>Åpne under Leverandører</a>
  </div>
{:else}
  <div class="flex flex-wrap items-start gap-3 mb-4">
    <div class="mr-auto">
      <h1 class="text-2xl font-bold">{party.name}</h1>
      <p class="opacity-70">
        Kunde nr. {party.party_no}{party.orgnr ? ", orgnr " + party.orgnr : ""}
      </p>
    </div>
    {#if canInvoice}
      <a class="btn btn-primary" href={"#/c/" + companyId + "/faktura/ny?kunde=" + party.party_no}>
        Ny faktura
      </a>
    {/if}
  </div>

  {#if !party.email}
    <div class="alert alert-warning alert-soft mb-4">
      <span>Kunden har ingen e-postadresse, så fakturaer kan ikke sendes på e-post.</span>
      {#if canEditContact}
        <button class="btn btn-sm" onclick={() => document.querySelector("#kontakt input[type=email]")?.focus()}>
          Legg til e-post
        </button>
      {/if}
    </div>
  {/if}

  <div class="stats stats-vertical sm:stats-horizontal border border-base-300 bg-base-100 w-full mb-6">
    <div class="stat">
      <div class="stat-title">Utestående</div>
      <div class="stat-value text-2xl tabular-nums">{kr(party.saldo_ore)}</div>
    </div>
    <div class="stat">
      <div class="stat-title">Forfalt</div>
      <div class="stat-value text-2xl tabular-nums {party.overdue_ore ? 'text-error' : ''}">
        {kr(party.overdue_ore)}
      </div>
    </div>
  </div>

  <!-- One column: the page's content area is too narrow for the invoice
       table beside a side card, whatever the viewport. -->
  <InvoiceList {companyId} party={party.party_no} />
  <section id="kontakt" aria-labelledby="kontakt-tittel" class="mb-6">
      <div class="card border border-base-300 bg-base-100 max-w-xl">
        <div class="card-body">
          <h2 id="kontakt-tittel" class="card-title">Kontaktinfo</h2>
          {#if canEditContact}
            {#key party.party_id}<PartyContact {companyId} {party} onSaved={load} />{/key}
          {:else}
            <dl class="grid gap-2 text-sm">
              <dt class="opacity-70">Adresse</dt><dd>{party.address || "–"}</dd>
              <dt class="opacity-70">E-post</dt><dd>{party.email || "–"}</dd>
              <dt class="opacity-70">Kontonummer</dt><dd>{party.bank_account || "–"}</dd>
            </dl>
          {/if}
        </div>
      </div>
  </section>

  <details id="reskontro" class="mt-2 scroll-mt-4" open={startOpen}>
    <summary class="cursor-pointer font-medium mb-3">Reskontro: poster og avstemming</summary>
    {#key party.party_id}<SubledgerItems {companyId} {party} onChanged={load} />{/key}
  </details>
{/if}
