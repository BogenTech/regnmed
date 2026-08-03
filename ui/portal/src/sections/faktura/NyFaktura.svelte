<script>
  import { untrack } from "svelte";
  import { post } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import DimSelect from "../../components/DimSelect.svelte";
  import VatSelect from "../../components/VatSelect.svelte";

  let { companyId, parties, products, dims, currencies, onDone } = $props();

  let partyNo = $state(untrack(() => parties[0]?.party_no || ""));
  let invoiceDate = $state(today());
  let dueDate = $state(today());
  let produkt = $state("");
  let valuta = $state("");
  let description = $state("");
  let quantity = $state("1");
  let unitPrice = $state("");
  let vatCode = $state("3");
  let avdeling = $state("");
  let prosjekt = $state("");

  // Et valgt produkt leverer pris og mva med mindre de overstyres.
  $effect(() => {
    vatCode = produkt ? "" : "3";
  });

  async function opprett(event) {
    event.preventDefault();
    try {
      const priceRaw = unitPrice.trim();
      if (!produkt && (!description.trim() || !priceRaw)) {
        throw new Error("velg produkt, eller skriv beskrivelse og pris");
      }
      const line = {
        produkt: produkt || null,
        description: description.trim() || null,
        quantity_milli: Math.round(Number(quantity.replace(",", ".")) * 1000),
        vat_code: vatCode || null,
        avdeling: avdeling || null,
        prosjekt: prosjekt || null,
      };
      if (priceRaw) line.unit_price_ore = parseKr(priceRaw);
      const issued = await post("/companies/" + companyId + "/invoices", {
        party_no: partyNo,
        invoice_date: invoiceDate,
        due_date: dueDate,
        valuta: valuta || null,
        lines: [line],
      });
      toast("Faktura " + issued.invoice_no + " opprettet (KID " + issued.kid + ")", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Ny faktura">
  {#if parties.length === 0}
    <p class="opacity-70">Opprett en kunde under Reskontro først.</p>
  {:else}
    <form class="grid gap-2 max-w-md" onsubmit={opprett}>
      <select class="select" bind:value={partyNo}>
        {#each parties as p (p.party_no)}
          <option value={p.party_no}>{p.party_no} {p.name}</option>
        {/each}
      </select>
      <div class="grid grid-cols-2 gap-2">
        <label class="fieldset">
          <span class="fieldset-legend">Fakturadato</span>
          <input type="date" class="input" bind:value={invoiceDate} />
        </label>
        <label class="fieldset">
          <span class="fieldset-legend">Forfall</span>
          <input type="date" class="input" bind:value={dueDate} />
        </label>
      </div>
      {#if products.length}
        <select class="select" bind:value={produkt}>
          <option value="">— fritekst —</option>
          {#each products as p (p.nummer)}
            <option value={p.nummer}>{p.nummer} {p.navn} ({kr(p.salgspris_ore)})</option>
          {/each}
        </select>
      {/if}
      {#if currencies.length}
        <select class="select" title="Fakturavaluta" bind:value={valuta}>
          <option value="">NOK</option>
          {#each currencies as c (c)}
            <option value={c}>{c}</option>
          {/each}
        </select>
      {/if}
      <input class="input" placeholder="Beskrivelse" bind:value={description} />
      <div class="grid grid-cols-3 gap-2">
        <input class="input" title="Antall" bind:value={quantity} />
        <input
          class="input"
          placeholder={produkt ? "Pris fra produkt" : "Pris (kr)"}
          bind:value={unitPrice}
        />
        <VatSelect produktvalg={products.length > 0} bind:value={vatCode} />
      </div>
      {#if dims.length}
        <div class="flex gap-2">
          <DimSelect {dims} kind="avdeling" bind:value={avdeling} />
          <DimSelect {dims} kind="prosjekt" bind:value={prosjekt} />
        </div>
      {/if}
      <button class="btn btn-primary">Opprett faktura</button>
    </form>
  {/if}
</Card>
