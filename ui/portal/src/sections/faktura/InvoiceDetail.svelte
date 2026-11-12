<script>
  // Én faktura: hva den inneholder, hvor den står, og det ene naturlige
  // neste steget som primærknapp (send, purre). Alle handlinger på en
  // faktura bor her i stedet for som fem knapper per rad i listen.
  import { api, post } from "../../lib/api.js";
  import { kr, today, formatDate, antallStr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import { sendDocument } from "../../lib/utsendelse.js";
  import Card from "../../components/Card.svelte";
  import InvoiceStatus, { daysOverdue } from "./InvoiceStatus.svelte";
  import PurringForm from "./PurringForm.svelte";

  let { companyId, invoiceId } = $props();

  let inv = $state(null);
  let error = $state(null);
  let sends = $state(null); // null = not known yet
  let showReminder = $state(false);

  const base = $derived("/companies/" + companyId + "/invoices/" + invoiceId);

  function load() {
    api(base)
      .then((r) => (inv = r))
      .catch((e) => (error = e.message));
    api(base + "/utsendelser")
      .then((r) => (sends = r.utsendelser.filter((u) => !u.reminder_id)))
      .catch(() => (sends = null));
  }

  $effect(() => {
    inv = null;
    error = null;
    sends = null;
    showReminder = false;
    load();
  });

  let title = $derived(inv ? (inv.is_credit_note ? "Kreditnota " : "Faktura ") + inv.invoice_no : "");
  let unpaid = $derived(inv && (inv.status === "open" || inv.status === "overdue"));
  let neverSent = $derived(sends !== null && sends.length === 0);

  // The single next step, named in words and offered as the primary
  // button.
  let next = $derived.by(() => {
    if (!inv) return null;
    // A reminder for an invoice the customer never received helps no one.
    if (inv.status === "overdue" && neverSent)
      return { text: "Fakturaen er forfalt, men er aldri sendt. Send den til kunden.", action: "send" };
    if (inv.status === "overdue")
      return { text: "Fakturaen er forfalt med " + daysOverdue(inv.due_date) + " dager. Send en purring.", action: "reminder" };
    if (inv.status === "open" && neverSent)
      return { text: "Fakturaen er ikke sendt ennå. Send den til kunden.", action: "send" };
    if (inv.status === "open")
      return { text: "Venter på betaling, forfall " + formatDate(inv.due_date) + ".", action: null };
    if (inv.status === "paid") return { text: "Fakturaen er betalt.", action: null };
    if (inv.status === "credited") return { text: "Fakturaen er kreditert.", action: null };
    return { text: "Kreditnotaen er ført mot fakturaen den krediterer.", action: null };
  });

  async function send() {
    if (await sendDocument(base + "/send")) load();
  }

  function pdf() {
    download(base + "/pdf", (inv.is_credit_note ? "kreditnota" : "faktura") + "-" + inv.invoice_no + ".pdf");
  }

  async function creditNote() {
    const ok = await bekreft(
      "Kreditnotaen opphever hele faktura " + inv.invoice_no + " (" + kr(inv.gross_ore) +
        "). Den bokføres og kan ikke angres.",
      { tittel: "Lag kreditnota", ok: "Lag kreditnota", farlig: true },
    );
    if (!ok) return;
    try {
      const credit = await post(base + "/credit-note", {});
      toast("Kreditnota " + credit.invoice_no + " er laget", true);
      location.hash = "#/c/" + companyId + "/faktura/" + credit.invoice_id;
    } catch (e) {
      toast(e.message, false);
    }
  }

  async function repeat() {
    const answer = await skjema(
      "Gjenta faktura " + inv.invoice_no,
      [
        {
          navn: "intervall",
          etikett: "Hvor ofte",
          type: "select",
          standard: "manedlig",
          valg: [
            ["manedlig", "Hver måned"],
            ["kvartalsvis", "Hvert kvartal"],
            ["arlig", "Hvert år"],
          ],
        },
        { navn: "neste", etikett: "Første faktura lages", type: "date", standard: today() },
      ],
      { ok: "Gjenta fakturaen" },
    );
    if (!answer) return;
    try {
      await post("/companies/" + companyId + "/invoice-templates", {
        from_invoice_id: inv.invoice_id,
        intervall: answer.intervall,
        neste_dato: answer.neste,
      });
      toast("Fakturaen gjentas automatisk", true);
      location.hash = "#/c/" + companyId + "/faktura/repeterende";
    } catch (e) {
      toast(e.message, false);
    }
  }
</script>

<nav class="mb-3">
  <a class="link link-hover text-sm" href={"#/c/" + companyId + "/faktura"}>← Alle fakturaer</a>
</nav>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if !inv}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else}
  <div class="flex flex-wrap items-center gap-3 mb-1">
    <h1 class="text-2xl font-bold">{title}</h1>
    <InvoiceStatus status={inv.status} dueDate={inv.due_date} />
  </div>
  <p class="opacity-70 mb-4">{inv.party_name}</p>

  <!-- Next step first, then every other action; wraps on a phone. -->
  <div class="card border border-base-300 bg-base-100 mb-6">
    <div class="card-body gap-3">
      <p>{next.text}</p>
      <div class="flex flex-wrap gap-2">
        {#if next.action === "send"}
          <button class="btn btn-primary" onclick={send}>Send til kunde</button>
        {:else if next.action === "reminder"}
          <button class="btn btn-primary" onclick={() => (showReminder = !showReminder)} aria-expanded={showReminder}>
            Purring
          </button>
        {/if}
        <button class="btn" onclick={pdf}>Last ned PDF</button>
        {#if next.action !== "send"}
          <button class="btn" onclick={send}>{neverSent ? "Send til kunde" : "Send på nytt"}</button>
        {/if}
        <button
          class="btn btn-ghost"
          title="Elektronisk faktura (PEPPOL BIS Billing 3.0)"
          onclick={() => download(base + "/ehf", "ehf-" + inv.invoice_no + ".xml")}
        >
          EHF
        </button>
        {#if !inv.is_credit_note}
          <button class="btn btn-ghost" onclick={repeat}>Gjenta</button>
        {/if}
        {#if unpaid && next.action !== "reminder"}
          <button class="btn btn-ghost" onclick={() => (showReminder = !showReminder)} aria-expanded={showReminder}>
            Purring
          </button>
        {/if}
        {#if !inv.is_credit_note && inv.status !== "credited"}
          <button class="btn btn-ghost text-error sm:ml-auto" onclick={creditNote}>Lag kreditnota</button>
        {/if}
      </div>
      {#if showReminder}
        <PurringForm {companyId} invoiceId={inv.invoice_id} onDone={load} />
      {/if}
    </div>
  </div>

  <div class="grid gap-6 lg:grid-cols-3">
    <div class="lg:col-span-2">
      <Card title="Linjer">
        <table class="table">
          <thead>
            <tr>
              <th scope="col">Beskrivelse</th>
              <th scope="col" class="text-right hidden sm:table-cell">Antall</th>
              <th scope="col" class="text-right hidden sm:table-cell">Pris</th>
              <th scope="col" class="text-right hidden sm:table-cell">Mva</th>
              <th scope="col" class="text-right">Beløp</th>
            </tr>
          </thead>
          <tbody>
            {#each inv.lines as l (l.line_no)}
              <tr>
                <td>
                  {l.description}
                  <span class="block text-sm opacity-70 sm:hidden">
                    {antallStr(l.quantity_milli)} × {kr(l.unit_price_ore)}
                  </span>
                  {#if l.prosjekt || l.avdeling}
                    <span class="block text-xs opacity-60">
                      {[l.prosjekt && "prosjekt " + l.prosjekt, l.avdeling && "avdeling " + l.avdeling].filter(Boolean).join(", ")}
                    </span>
                  {/if}
                </td>
                <td class="text-right tabular-nums hidden sm:table-cell">{antallStr(l.quantity_milli)}</td>
                <td class="text-right tabular-nums hidden sm:table-cell">{kr(l.unit_price_ore)}</td>
                <td class="text-right tabular-nums hidden sm:table-cell">{kr(l.vat_ore)}</td>
                <td class="text-right tabular-nums whitespace-nowrap">{kr(l.net_ore)}</td>
              </tr>
            {/each}
          </tbody>
          <tfoot>
            <tr>
              <th scope="row" colspan="4" class="text-right hidden sm:table-cell">Netto</th>
              <th scope="row" class="sm:hidden">Netto</th>
              <td class="text-right tabular-nums">{kr(inv.net_ore)}</td>
            </tr>
            <tr>
              <th scope="row" colspan="4" class="text-right hidden sm:table-cell">Mva</th>
              <th scope="row" class="sm:hidden">Mva</th>
              <td class="text-right tabular-nums">{kr(inv.vat_ore)}</td>
            </tr>
            <tr class="text-base">
              <th scope="row" colspan="4" class="text-right hidden sm:table-cell">Totalt{inv.valuta ? " (" + inv.valuta + ")" : ""}</th>
              <th scope="row" class="sm:hidden">Totalt</th>
              <td class="text-right tabular-nums font-bold">{kr(inv.net_ore + inv.vat_ore)}</td>
            </tr>
          </tfoot>
        </table>
      </Card>
    </div>

    <Card title="Detaljer">
      <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
        <dt class="opacity-70">Kunde</dt>
        <dd>{inv.party_name} <span class="opacity-60">({inv.party_no})</span></dd>
        <dt class="opacity-70">Fakturadato</dt>
        <dd>{formatDate(inv.invoice_date)}</dd>
        {#if !inv.is_credit_note}
          <dt class="opacity-70">Forfall</dt>
          <dd>{formatDate(inv.due_date)}</dd>
        {/if}
        {#if inv.delivery_date}
          <dt class="opacity-70">Levert</dt>
          <dd>{formatDate(inv.delivery_date)}{inv.delivery_place ? ", " + inv.delivery_place : ""}</dd>
        {/if}
        <dt class="opacity-70">KID</dt>
        <dd class="font-mono">{inv.kid}</dd>
        <dt class="opacity-70">Utestående</dt>
        <dd class="tabular-nums">{kr(inv.remaining_ore)}</dd>
        {#if inv.credits}
          <dt class="opacity-70">Krediterer</dt>
          <dd>
            <a class="link" href={"#/c/" + companyId + "/faktura/" + inv.credits.invoice_id}>
              Faktura {inv.credits.invoice_no}
            </a>
          </dd>
        {/if}
        {#if inv.credited_by}
          <dt class="opacity-70">Kreditert av</dt>
          <dd>
            <a class="link" href={"#/c/" + companyId + "/faktura/" + inv.credited_by.invoice_id}>
              Kreditnota {inv.credited_by.invoice_no}
            </a>
          </dd>
        {/if}
        <dt class="opacity-70">Sendt</dt>
        <dd>
          {#if sends === null}
            –
          {:else if sends.length === 0}
            Ikke sendt
          {:else}
            {#each sends as s (s.utsendelse_id)}
              <span class="block">{formatDate(s.sent_at)} til {s.to}</span>
            {/each}
          {/if}
        </dd>
        <dt class="opacity-70">Laget av</dt>
        <dd>{inv.created_by}</dd>
      </dl>
    </Card>
  </div>
{/if}
