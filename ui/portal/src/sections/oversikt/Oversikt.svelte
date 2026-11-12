<script>
  // Oversikten svarer på «hva må jeg gjøre i dag?» før «hvordan går
  // det?»: snarveier, så det som venter, så tallene. Verktøy og
  // innstillinger bor i sine egne seksjoner (valutakurser i Bank,
  // forankringen under Rapporter → Revisjon).
  import { api } from "../../lib/api.js";
  import { kr, krWhole, formatDate } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import Ikon from "../../components/Ikon.svelte";
  import Nokkeltall from "./Nokkeltall.svelte";
  import ImportKort from "./ImportKort.svelte";
  import InviterKort from "./InviterKort.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const c = "/companies/" + id;
    let incomplete = false;
    const optional = (promise) =>
      promise.catch(() => {
        incomplete = true;
        return null;
      });
    // The page draws as soon as these three are in; nothing slower may
    // hold it, and a refused one (a custom role without that right)
    // leaves its part out instead of the whole page.
    const [open, recent, overdue] = await Promise.all([
      optional(api(c + "/invoices?open=true")),
      optional(api(c + "/vouchers?limit=8&lines=true")),
      optional(api(c + "/invoices/overdue")),
    ]);
    // The user may have switched company while these were in flight.
    if (id !== companyId) return;
    data = {
      open: open?.invoices ?? null,
      recent: recent?.vouchers ?? null,
      overdue: overdue?.invoices ?? [],
      importOpen: recent?.total === 0,
      hasHistory: recent?.total > 0,
      figures: null,
      inbox: [],
      unmatched: [],
      anchor: undefined, // undefined = not known yet, null = never anchored
      pending: 0,
      incomplete,
    };
    const current = data;

    // The rest fills in as it arrives, each on its own.
    const fill = (path, set) => {
      current.pending++;
      api(c + path)
        .then((r) => data === current && set(r))
        .catch(() => (current.incomplete = true))
        .finally(() => current.pending--);
    };
    fill("/reports/nokkeltall", (r) => (current.figures = r));
    fill("/inbox?status=ny", (r) => (current.inbox = r.documents));
    fill("/bank/reconciliation?account=1920", (r) => (current.unmatched = r.unmatched_bank));
    fill("/anchors?limit=1", (r) => (current.anchor = r.anchors[0] ?? null));

    // Import stays open while the ledger holds nothing but imported
    // history. Only a ledger whose newest vouchers are all IMP needs the
    // full list to tell.
    if (recent?.total > 0 && recent.vouchers.every((v) => v.journal === "IMP")) {
      if (recent.total <= recent.vouchers.length) current.importOpen = true;
      else
        fill("/vouchers", (r) => (current.importOpen = r.vouchers.every((v) => v.journal === "IMP")));
    }
  }

  function reload() {
    load(companyId);
  }

  $effect(() => {
    data = null;
    load(companyId);
  });

  const sum = (rows) => rows.reduce((s, r) => s + r.remaining_ore, 0);
  const plural = (n, one, many) => n + " " + (n === 1 ? one : many);
  const daysUntil = (iso) =>
    Math.round((Date.parse(iso + "T00:00") - new Date().setHours(0, 0, 0, 0)) / 864e5);
  // A voucher's size is its debit side; the lines always balance.
  const voucherTotal = (v) => (v.lines || []).reduce((s, l) => s + Math.max(l.amount_ore, 0), 0);
  // "Årstermin 2026" from the server reads mid-sentence as "årstermin 2026".
  const lowerFirst = (s) => s.charAt(0).toLowerCase() + s.slice(1);

  const href = (section) => "#/c/" + companyId + "/" + section;
  let openSum = $derived(data?.open ? sum(data.open) : 0);
  let overdueSum = $derived(data ? sum(data.overdue) : 0);

  // What is waiting on the user, most pressing first. Each line names
  // the work and links straight to where it is done.
  let todo = $derived.by(() => {
    if (!data) return [];
    const lines = [];
    if (data.overdue.length) {
      lines.push({
        section: "faktura",
        text: plural(data.overdue.length, "forfalt faktura", "forfalte fakturaer"),
        detail: krWhole(overdueSum) + " kr ubetalt",
        urgent: true,
      });
    }
    const deadline = data.figures?.frister?.[0];
    if (deadline) {
      const days = daysUntil(deadline.frist);
      lines.push({
        section: "mva",
        text: "Lever mva-melding for " + lowerFirst(deadline.label),
        detail:
          "frist " + formatDate(deadline.frist) + (days === 0 ? ", i dag" : ", om " + plural(days, "dag", "dager")),
        urgent: days <= 7,
      });
    }
    if (data.inbox.length) {
      lines.push({
        section: "bilag",
        text: plural(data.inbox.length, "dokument", "dokumenter") + " i innboksen",
        detail: "venter på bokføring",
      });
    }
    if (data.unmatched.length) {
      lines.push({
        section: "bank",
        text: plural(data.unmatched.length, "banktransaksjon", "banktransaksjoner") + " uten match",
        detail: "avstem mot hovedboken",
      });
    }
    return lines;
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <div class="flex flex-wrap gap-2 mb-6">
    <a class="btn btn-primary btn-sm" href={href("faktura")}><Ikon navn="faktura" />Ny faktura</a>
    <a class="btn btn-sm" href={href("bilag")}><Ikon navn="bilag" />Last opp bilag</a>
    <a class="btn btn-sm" href={href("timer")}><Ikon navn="timer" />Registrer timer</a>
  </div>

  <!-- Import stays available while the ledger holds nothing but imported
       history: systems like Conta export one SAF-T file per fiscal year,
       so the years arrive one upload at a time. The server enforces the
       rule; this condition only mirrors it. -->
  {#if data.importOpen}
    <ImportKort {companyId} onDone={reload} harHistorikk={data.hasHistory} />
  {/if}

  <Card title="Til oppfølging">
    {#if todo.length}
      <ul class="-mx-2">
        {#each todo as l (l.section)}
          <li>
            <a class="flex items-center gap-3 rounded-box px-2 py-2 hover:bg-base-200" href={href(l.section)}>
              <span class={l.urgent ? "text-error" : "opacity-60"}><Ikon navn={l.section} /></span>
              <span class="flex-1 min-w-0">
                <span class="font-medium {l.urgent ? 'text-error' : ''}">{l.text}</span>
                <span class="block text-sm opacity-70">{l.detail}</span>
              </span>
            </a>
          </li>
        {/each}
      </ul>
    {:else if !data.pending && !data.incomplete}
      <p class="text-sm opacity-70">Ingenting venter på deg nå.</p>
    {/if}
    {#if data.pending}
      <span class="loading loading-dots loading-sm opacity-60" aria-label="Henter mer"></span>
    {:else if data.incomplete}
      <p class="text-sm opacity-70">Noe kunne ikke hentes, så listen kan være ufullstendig.</p>
    {/if}
  </Card>

  {#if data.open || data.figures}
    <!-- Stack on narrow screens instead of scrolling sideways. -->
    <div class="stats stats-vertical md:stats-horizontal border border-base-300 bg-base-100 w-full mb-6">
      {#if data.open}
        <a class="stat hover:bg-base-200" href={href("faktura")}>
          <div class="stat-title">Utestående fra kunder</div>
          <div class="stat-value text-2xl">{krWhole(openSum)}</div>
          <div class="stat-desc">
            {#if overdueSum}
              <span class="text-error">{krWhole(overdueSum)} forfalt</span>
              av {plural(data.open.length, "faktura", "fakturaer")}
            {:else}
              {plural(data.open.length, "åpen faktura", "åpne fakturaer")}, ingen forfalt
            {/if}
          </div>
        </a>
      {/if}
      {#if data.figures}
        <a class="stat hover:bg-base-200" href={href("rapporter/resultat")}>
          <div class="stat-title">Resultat hittil i {data.figures.year}</div>
          <div class="stat-value text-2xl {data.figures.resultat_hittil_ore < 0 ? 'text-error' : ''}">
            {krWhole(data.figures.resultat_hittil_ore)}
          </div>
          <div class="stat-desc">Samme dato i fjor: {krWhole(data.figures.resultat_fjor_ore)}</div>
        </a>
        <div
          class="stat"
          title={"Bank " + kr(data.figures.likviditet.bank_ore) +
            " + kunder " + kr(data.figures.likviditet.kundefordringer_ore) +
            " − leverandører " + kr(data.figures.likviditet.leverandorgjeld_ore) +
            " − mva " + kr(Math.max(data.figures.likviditet.mva_netto_ore, 0))}
        >
          <div class="stat-title">Disponibelt</div>
          <div class="stat-value text-2xl {data.figures.likviditet.disponibelt_ore < 0 ? 'text-error' : ''}">
            {krWhole(data.figures.likviditet.disponibelt_ore)}
          </div>
          <div class="stat-desc">bank og kunder, minus leverandører og mva</div>
        </div>
      {/if}
    </div>
  {/if}

  {#if data.figures?.maaneder.some((v) => v !== 0)}
    <Nokkeltall tall={data.figures} />
  {/if}

  {#if data.recent}
    <Card title="Siste bilag">
      {#if data.recent.length}
        <table class="table table-sm">
          <thead>
            <tr><th class="hidden sm:table-cell">Bilag</th><th>Dato</th><th>Tekst</th><th class="text-right">Beløp</th></tr>
          </thead>
          <tbody>
            {#each data.recent as v (v.voucher_id)}
              <tr>
                <td class="hidden sm:table-cell whitespace-nowrap">{v.voucher}</td>
                <td class="whitespace-nowrap">{formatDate(v.date)}</td>
                <td>{v.description}</td>
                <td class="text-right tabular-nums whitespace-nowrap">{kr(voucherTotal(v))}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        <a class="link text-sm" href={href("hovedbok")}>Se alle bilag i hovedboken</a>
      {:else}
        <p class="text-sm opacity-70">
          Ingen bilag ennå. Lag en <a class="link" href={href("faktura")}>faktura</a> eller
          <a class="link" href={href("bilag")}>last opp et bilag</a>, så dukker de opp her.
        </p>
      {/if}
    </Card>
  {/if}

  <InviterKort {companyId} />

  <p class="text-sm opacity-70">
    <!-- Only a statement the server confirmed: while loading, or when the
         request was refused, say nothing about the anchoring. -->
    {#if data.anchor}
      Hovedboken ble sist forankret {formatDate(data.anchor.created_at)}{data.anchor.witnesses.length
        ? " og bevitnet eksternt"
        : ""}.
    {:else if data.anchor === null}
      Hovedboken er ikke forankret ennå. Det skjer automatisk hver natt.
    {/if}
    <a class="link" href={href("rapporter/revisjon")}>Kontroller at ingenting er endret</a>
  </p>
{/if}
