<script>
  // Tilbud→ordre→faktura (#31): utenfor hovedboken, redigerbar til
  // akseptert; enveis statuser, én ordre per tilbud. The one-way steps —
  // rejecting a tilbud and invoicing an ordre — ask first; invoicing
  // lands on the new invoice, where sending it is the next step.
  import { api, post } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import Card from "../../components/Card.svelte";
  import QuoteForm from "./QuoteForm.svelte";

  let { companyId, quotes, orders, parties, products, onDone } = $props();

  const STATUS = {
    utkast: ["Utkast", "badge-ghost"],
    sendt: ["Sendt", "badge-info badge-soft"],
    akseptert: ["Akseptert", "badge-success badge-soft"],
    avslatt: ["Avslått", "badge-ghost"],
    bekreftet: ["Bekreftet", "badge-warning badge-soft"],
    fakturert: ["Fakturert", "badge-success badge-soft"],
  };
  const editable = (q) => q.status === "utkast" || q.status === "sendt";

  // null = closed, "new" = new tilbud, or the tilbud being edited.
  let form = $state(null);

  async function edit(q) {
    try {
      const { lines } = await api("/companies/" + companyId + "/quotes/" + q.id);
      form = { ...q, lines };
    } catch (e) {
      toast(e.message, false);
    }
  }

  function saved() {
    form = null;
    onDone();
  }

  async function settStatus(q, status) {
    if (status === "avslatt") {
      const ok = await bekreft("Et avslått tilbud kan ikke tas opp igjen — da lager du et nytt.", {
        tittel: "Merke T-" + q.doc_no + " som avslått?",
        ok: "Merk som avslått",
        farlig: true,
      });
      if (!ok) return;
    }
    try {
      await post("/companies/" + companyId + "/quotes/" + q.id + "/status", { status });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function tilOrdre(q) {
    try {
      const made = await post("/companies/" + companyId + "/quotes/" + q.id + "/order", {});
      toast("Ordre O-" + made.doc_no + " er laget", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function tilFaktura(o) {
    const ok = await bekreft(
      "Faktura til " + o.party_name + " for ordre O-" + o.doc_no + ", " + kr(o.netto_ore) +
        " kr pluss mva. En utstedt faktura kan ikke endres — feil rettes med kreditnota.",
      { tittel: "Utstede fakturaen?", ok: "Utsted faktura" },
    );
    if (!ok) return;
    try {
      const issued = await post("/companies/" + companyId + "/orders/" + o.id + "/invoice", {});
      toast("Faktura " + issued.invoice_no + " er utstedt", true);
      location.hash = "#/c/" + companyId + "/faktura/" + issued.invoice_id;
    } catch (error) {
      toast(error.message, false);
    }
  }

  const pdf = (kind, d) =>
    download(
      "/companies/" + companyId + "/" + (kind === "tilbud" ? "quotes/" : "orders/") + d.id + "/pdf",
      kind + "-" + d.doc_no + ".pdf",
    );
</script>

<p class="text-sm opacity-70 mb-4">
  Kjeden før fakturaen, utenfor hovedboken. Et tilbud kan endres til det er akseptert eller avslått;
  et akseptert tilbud blir en ordre, og ordren faktureres.
</p>

<Card title="Tilbud">
  {#if form}
    {#key form}
      <QuoteForm {companyId} {parties} {products} quote={form === "new" ? null : form} onDone={saved} onCancel={() => (form = null)} />
    {/key}
  {:else if parties.length}
    <div><button class="btn btn-sm btn-primary" onclick={() => (form = "new")}>Nytt tilbud</button></div>
  {:else}
    <p class="text-sm">
      Du trenger en kunde før du kan lage et tilbud.
      <a class="link" href={"#/c/" + companyId + "/kunder/ny"}>Opprett kunde</a>
    </p>
  {/if}

  {#if quotes.length}
    <div class="overflow-x-auto">
      <table class="table table-sm mt-2">
        <caption class="sr-only">Tilbud, nyeste først.</caption>
        <thead>
          <tr>
            <th scope="col">Tilbud</th><th scope="col" class="hidden sm:table-cell">Dato</th>
            <th scope="col">Status</th><th scope="col" class="text-right">Netto</th>
            <th scope="col"><span class="sr-only">Handlinger</span></th>
          </tr>
        </thead>
        <tbody>
          {#each quotes as q (q.id)}
            <tr>
              <td>
                <span class="font-medium">T-{q.doc_no}</span>
                <span class="block text-sm opacity-70">{q.party_name}</span>
              </td>
              <td class="hidden sm:table-cell whitespace-nowrap">{formatDate(q.doc_date)}</td>
              <td><span class="badge badge-sm whitespace-nowrap {STATUS[q.status]?.[1] || 'badge-ghost'}">{STATUS[q.status]?.[0] || q.status}</span></td>
              <td class="text-right tabular-nums whitespace-nowrap">{kr(q.netto_ore)}</td>
              <td>
                <div class="flex flex-wrap gap-1 justify-end">
                  {#if q.status === "akseptert"}
                    <button class="btn btn-xs btn-primary" onclick={() => tilOrdre(q)}>Lag ordre</button>
                  {/if}
                  {#if editable(q)}
                    <button class="btn btn-xs btn-outline" onclick={() => settStatus(q, "akseptert")}>Akseptert</button>
                  {/if}
                  {#if q.status === "utkast"}
                    <button class="btn btn-xs btn-ghost" onclick={() => settStatus(q, "sendt")}>Merk som sendt</button>
                  {/if}
                  {#if editable(q)}
                    <button class="btn btn-xs btn-ghost" aria-label={"Endre tilbud T-" + q.doc_no} onclick={() => edit(q)}>Endre</button>
                    <button class="btn btn-xs btn-ghost" onclick={() => settStatus(q, "avslatt")}>Avslått</button>
                  {/if}
                  <button class="btn btn-xs btn-ghost" aria-label={"Last ned tilbud T-" + q.doc_no + " som PDF"} onclick={() => pdf("tilbud", q)}>PDF</button>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if !form}
    <p class="text-sm opacity-70 mt-2">Ingen tilbud ennå.</p>
  {/if}
</Card>

<Card title="Ordre">
  {#if orders.length}
    <div class="overflow-x-auto">
      <table class="table table-sm">
        <caption class="sr-only">Ordre, nyeste først.</caption>
        <thead>
          <tr>
            <th scope="col">Ordre</th><th scope="col" class="hidden sm:table-cell">Dato</th>
            <th scope="col">Status</th><th scope="col" class="text-right">Netto</th>
            <th scope="col"><span class="sr-only">Handlinger</span></th>
          </tr>
        </thead>
        <tbody>
          {#each orders as o (o.id)}
            <tr>
              <td>
                <span class="font-medium">O-{o.doc_no}</span>
                {#if o.tilbud_no}<span class="opacity-60">fra T-{o.tilbud_no}</span>{/if}
                <span class="block text-sm opacity-70">{o.party_name}</span>
              </td>
              <td class="hidden sm:table-cell whitespace-nowrap">{formatDate(o.doc_date)}</td>
              <td>
                <span class="badge badge-sm whitespace-nowrap {STATUS[o.status]?.[1] || 'badge-ghost'}">{STATUS[o.status]?.[0] || o.status}</span>
                {#if o.invoice_no}<span class="block text-xs opacity-70">faktura {o.invoice_no}</span>{/if}
              </td>
              <td class="text-right tabular-nums whitespace-nowrap">{kr(o.netto_ore)}</td>
              <td>
                <div class="flex flex-wrap gap-1 justify-end">
                  {#if o.status === "bekreftet"}
                    <button class="btn btn-xs btn-primary" onclick={() => tilFaktura(o)}>Lag faktura</button>
                  {/if}
                  <button class="btn btn-xs btn-ghost" aria-label={"Last ned ordre O-" + o.doc_no + " som PDF"} onclick={() => pdf("ordre", o)}>PDF</button>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <p class="text-sm opacity-70">Ingen ordre ennå. Et akseptert tilbud blir en ordre med «Lag ordre».</p>
  {/if}
</Card>
