<script>
  // Tilbud→ordre→faktura (#31): utenfor hovedboken, redigerbar til
  // akseptert; enveis statuser, én ordre per tilbud.
  import { untrack } from "svelte";
  import { post } from "../../lib/api.js";
  import { kr, parseKr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import Card from "../../components/Card.svelte";
  import VatSelect from "../../components/VatSelect.svelte";

  let { companyId, quotes, orders, parties, products, onDone } = $props();

  const STATUS_NAVN = {
    utkast: "utkast", sendt: "sendt", akseptert: "akseptert", avslatt: "avslått",
    bekreftet: "bekreftet", fakturert: "fakturert",
  };

  let party = $state(untrack(() => parties[0]?.party_no || ""));
  let produkt = $state("");
  let desc = $state("");
  let price = $state("");
  let vat = $state("3");

  async function nyttTilbud() {
    try {
      if (!produkt && (!desc.trim() || !price.trim())) {
        throw new Error("velg produkt, eller skriv beskrivelse og pris");
      }
      const line = {
        produkt: produkt || null,
        description: desc.trim() || null,
        vat_code: vat || null,
      };
      if (price.trim()) line.unit_price_ore = parseKr(price);
      const made = await post("/companies/" + companyId + "/quotes", {
        party_no: party,
        lines: [line],
      });
      toast("Tilbud T-" + made.doc_no + " opprettet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function settStatus(quote, status) {
    try {
      await post("/companies/" + companyId + "/quotes/" + quote.id + "/status", { status });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function tilOrdre(quote) {
    try {
      const made = await post("/companies/" + companyId + "/quotes/" + quote.id + "/order", {});
      toast("Ordre O-" + made.doc_no + " opprettet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function tilFaktura(order) {
    try {
      const issued = await post("/companies/" + companyId + "/orders/" + order.id + "/invoice", {});
      toast("Faktura " + issued.invoice_no + " opprettet (KID " + issued.kid + ")", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Tilbud og ordre">
  <p class="text-sm opacity-70 mb-2">
    Kjeden før fakturaen — utenfor hovedboken, redigerbar til den er akseptert. Akseptert tilbud
    blir ordre; ordre faktureres gjennom den ordinære fakturaflyten.
  </p>
  {#if parties.length}
    <div class="flex flex-wrap gap-2 mb-3 items-end">
      <select class="select select-sm" bind:value={party}>
        {#each parties as p (p.party_no)}
          <option value={p.party_no}>{p.party_no} {p.name}</option>
        {/each}
      </select>
      {#if products.length}
        <select class="select select-sm" bind:value={produkt}>
          <option value="">— fritekst —</option>
          {#each products as p (p.nummer)}
            <option value={p.nummer}>{p.nummer} {p.navn} ({kr(p.salgspris_ore)})</option>
          {/each}
        </select>
      {/if}
      <input class="input input-sm" placeholder="Beskrivelse" bind:value={desc} />
      <input class="input input-sm w-28" placeholder="Pris (kr)" bind:value={price} />
      <VatSelect cls="select select-sm" produktvalg={products.length > 0} bind:value={vat} />
      <button class="btn btn-sm" onclick={nyttTilbud}>Nytt tilbud</button>
    </div>
  {/if}
  {#if quotes.length}
    <table class="table table-sm mb-3">
      <thead>
        <tr><th>Nr</th><th>Kunde</th><th>Dato</th><th>Status</th><th class="text-right">Netto</th><th></th></tr>
      </thead>
      <tbody>
        {#each quotes as q (q.id)}
          <tr>
            <td>T-{q.doc_no}</td>
            <td>{q.party_name}</td>
            <td>{q.doc_date}</td>
            <td>{STATUS_NAVN[q.status] || q.status}</td>
            <td class="text-right">{kr(q.netto_ore)}</td>
            <td>
              <button
                class="btn btn-xs btn-ghost"
                onclick={() =>
                  download(
                    "/companies/" + companyId + "/quotes/" + q.id + "/pdf",
                    "tilbud-" + q.doc_no + ".pdf",
                  )}
              >
                PDF
              </button>
              {#if q.status === "utkast"}
                <button class="btn btn-xs btn-ghost" onclick={() => settStatus(q, "sendt")}>
                  Merk sendt
                </button>
              {/if}
              {#if q.status === "utkast" || q.status === "sendt"}
                <button class="btn btn-xs btn-outline" onclick={() => settStatus(q, "akseptert")}>
                  Akseptert
                </button>
                <button class="btn btn-xs btn-ghost" onclick={() => settStatus(q, "avslatt")}>
                  Avslått
                </button>
              {/if}
              {#if q.status === "akseptert"}
                <button class="btn btn-xs btn-primary" onclick={() => tilOrdre(q)}>→ Ordre</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  {#if orders.length}
    <table class="table table-sm">
      <thead>
        <tr><th>Ordre</th><th>Kunde</th><th>Dato</th><th>Status</th><th class="text-right">Netto</th><th></th></tr>
      </thead>
      <tbody>
        {#each orders as o (o.id)}
          <tr>
            <td>
              O-{o.doc_no}
              {#if o.tilbud_no}<span class="opacity-60">(T-{o.tilbud_no})</span>{/if}
            </td>
            <td>{o.party_name}</td>
            <td>{o.doc_date}</td>
            <td>
              {STATUS_NAVN[o.status] || o.status}
              {#if o.invoice_no}<span class="opacity-60">(faktura {o.invoice_no})</span>{/if}
            </td>
            <td class="text-right">{kr(o.netto_ore)}</td>
            <td>
              <button
                class="btn btn-xs btn-ghost"
                onclick={() =>
                  download(
                    "/companies/" + companyId + "/orders/" + o.id + "/pdf",
                    "ordre-" + o.doc_no + ".pdf",
                  )}
              >
                PDF
              </button>
              {#if o.status === "bekreftet"}
                <button class="btn btn-xs btn-primary" onclick={() => tilFaktura(o)}>
                  → Faktura
                </button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</Card>
