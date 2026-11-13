<script>
  // Mva: the term being filed, what makes it up, and what is owed. The
  // page opens on the latest term that has ENDED; year and term live in
  // the address (#/c/{id}/mva?year=&termin=&kode=) so a term, or a code
  // within it, can be linked. Terms follow the company's registered
  // terminordning (#51) — the server refuses any other — and every code
  // opens the postings behind it, so the return can be checked before it
  // is filed. Amounts are shown as the return reads them: what raises
  // the amount owed positive, a deduction negative (payable_effect_ore,
  // the same rule as the mva-melding).
  import { untrack } from "svelte";
  import { api } from "../../lib/api.js";
  import { kr, formatDate, today } from "../../lib/format.js";
  import { download } from "../../lib/download.js";
  import { harRett } from "../../lib/me.svelte.js";
  import Terminordning from "./Terminordning.svelte";
  import { termName, defaultTerm, deadlineText } from "./mvaTerms.js";

  let { companyId, query = "" } = $props();

  const base = $derived("/companies/" + companyId);
  const todayIso = today();
  const initial = new URLSearchParams(untrack(() => query));

  let scheme = $state(null);
  let settings = $state(null);
  let year = $state(null);
  let termin = $state(null);
  let open = $state(initial.get("kode"));
  let report = $state(undefined); // undefined = loading, null = failed
  let reportError = $state(null);
  let postings = $state({}); // code -> response | null (loading)
  let error = $state(null);

  $effect(() => {
    Promise.all([api(base + "/mva/terminordning"), api(base + "/settings").catch(() => null)])
      .then(([s, st]) => {
        scheme = s;
        settings = st;
        const def = defaultTerm(s.antall_perioder, todayIso);
        year = Number(initial.get("year")) || def.year;
        termin = s.antall_perioder === 1 ? 1 : Number(initial.get("termin")) || def.termin;
      })
      .catch((e) => (error = e.message));
  });

  // The number of terms in the year shown: the scheme in force on its
  // 1 January (a company can move between schemes). Fetched per year, and
  // the term is clamped to it BEFORE the report is asked for, so stepping
  // into a year with fewer terms never asks for one that does not exist.
  let count = $state(null);
  let countYear = $state(null);
  $effect(() => {
    if (year === null) return;
    const y = year;
    api(base + "/mva/terminordning?year=" + y)
      .then((s) => {
        if (y !== year) return;
        if (termin > s.antall_perioder) termin = s.antall_perioder;
        count = s.antall_perioder;
        countYear = y;
      })
      .catch((e) => (error = e.message));
  });

  // An address changed from outside (a link, the address bar) moves the
  // page; the page's own replaceState does not reach `query`.
  $effect(() => {
    const q = new URLSearchParams(query);
    untrack(() => {
      if (year === null) return;
      const y = Number(q.get("year"));
      const t = Number(q.get("termin"));
      if (y && y !== year) year = y;
      if (t && t !== termin) termin = t;
      if (q.has("kode") && q.get("kode") !== open) open = q.get("kode");
    });
  });

  let seq = 0;
  $effect(() => {
    if (year === null || countYear !== year) return;
    const mine = ++seq;
    report = undefined;
    reportError = null;
    postings = {};
    api(base + "/reports/mva?year=" + year + "&termin=" + termin)
      .then((r) => mine === seq && (report = r))
      .catch((e) => {
        if (mine !== seq) return;
        report = null;
        reportError = e.message;
      });
  });

  // Year, term and the open code in the address.
  $effect(() => {
    if (year === null) return;
    const p = new URLSearchParams({ year, termin });
    if (open) p.set("kode", open);
    const hash = "#/c/" + companyId + "/mva?" + p;
    if (location.hash !== hash) history.replaceState(null, "", hash);
  });

  function step(delta) {
    year += delta;
    open = null;
  }
  function pick(t) {
    termin = t;
    open = null;
  }
  function toggle(code) {
    open = open === code ? null : code;
  }
  $effect(() => {
    const code = open;
    if (!code || !report || postings[code] !== undefined) return;
    postings = { ...postings, [code]: null };
    api(base + "/reports/mva/postings?year=" + report.year + "&termin=" + report.termin + "&kode=" + encodeURIComponent(code))
      .then((r) => (postings = { ...postings, [code]: r }))
      .catch((e) => (postings = { ...postings, [code]: { error: e.message } }));
  });

  // Registration: when the current, unbroken registration began.
  let registeredFrom = $derived.by(() => {
    if (!settings?.mva_registrert) return null;
    const rows = [...(settings.registrering_historikk || [])].sort((a, b) => a.valid_from.localeCompare(b.valid_from));
    let from = null;
    for (const r of rows) from = r.mva_registrert ? (from ?? r.valid_from) : null;
    return from;
  });

  const SCHEME = { "to-maneder": "to-månedersterminer", arlig: "årstermin", primaernaering: "årstermin (primærnæring)" };
  const longDate = (iso) =>
    new Date(iso + "T12:00").toLocaleDateString("nb-NO", { day: "numeric", month: "long", year: "numeric" });
  const NOTE = {
    omvendt_med_fradrag: "beregnes og trekkes fra; netto null",
    kostnadsmarkor: "rapporteres ikke",
  };
  let deadlinePassed = $derived(report ? report.frist < todayIso : false);
</script>

<h1 class="text-2xl font-bold mb-2">Mva</h1>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if !scheme || year === null || count === null}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else}
  <p class="opacity-80 mb-4">
    {#if settings && !settings.mva_registrert}
      <span class="text-warning font-medium">Ikke mva-registrert.</span>
    {:else if registeredFrom}
      Mva-registrert fra {formatDate(registeredFrom)}.
    {:else if settings}
      Mva-registrert.
    {/if}
    Terminordning: {SCHEME[scheme.ordning] || scheme.ordning}.
  </p>
  {#if settings && !settings.mva_registrert}
    <div class="alert alert-warning alert-soft mb-4" role="status">
      <span>
        Et selskap som ikke er mva-registrert kan verken kreve inn eller trekke fra mva, og bilag med mva-kode avvises når de
        bokføres. Er selskapet registrert, oppdater statusen under Administrasjon → Firmaopplysninger.
      </span>
    </div>
  {/if}

  <div class="flex flex-wrap items-end gap-3 mb-4">
    <div class="join" role="group" aria-label="År">
      <button class="btn btn-sm join-item" aria-label={"Forrige år, " + (year - 1)} onclick={() => step(-1)}>‹</button>
      <span class="btn btn-sm join-item pointer-events-none tabular-nums" aria-live="polite">{year}</span>
      <button class="btn btn-sm join-item" aria-label={"Neste år, " + (year + 1)} onclick={() => step(1)}>›</button>
    </div>
    {#if count > 1}
      <nav class="join flex-wrap" aria-label={"Terminer i " + year}>
        {#each Array.from({ length: count }, (_, i) => i + 1) as t (t)}
          <a
            class="join-item btn btn-sm {t === termin ? 'btn-primary' : ''}"
            href={"#/c/" + companyId + "/mva?year=" + year + "&termin=" + t}
            aria-current={t === termin ? "page" : undefined}
            onclick={(e) => {
              e.preventDefault();
              pick(t);
            }}
          >
            {termName(count, t)}
          </a>
        {/each}
      </nav>
    {/if}
  </div>

  {#if report === undefined}
    <div class="flex justify-center p-8"><span class="loading loading-spinner"></span></div>
  {:else if report === null}
    <div class="alert alert-error mb-6" role="alert"><span>Spesifikasjonen kunne ikke hentes: {reportError}</span></div>
  {:else}
    <section class="card border border-base-300 bg-base-100 mb-6" aria-labelledby="spes-tittel">
      <div class="card-body gap-3">
        <div class="flex flex-wrap items-baseline gap-x-4 gap-y-1">
          <h2 id="spes-tittel" class="card-title mr-auto">
            {termName(report.antall_perioder, report.termin)} {report.year}
          </h2>
          <span class={deadlinePassed ? "opacity-70" : "font-medium"}>
            Frist {longDate(report.frist)}, {deadlineText(report.frist, todayIso)}
          </span>
        </div>

        {#if report.lines.length === 0}
          <p class="opacity-70">Ingen posteringer med mva-kode i terminen.</p>
        {:else}
          <div class="overflow-x-auto">
            <table class="table table-sm">
              <caption class="sr-only">Mva-spesifikasjon per kode. Åpne en kode for å se posteringene bak.</caption>
              <thead>
                <tr>
                  <th scope="col">Kode</th>
                  <th scope="col" class="text-right hidden sm:table-cell">Grunnlag</th>
                  <th scope="col" class="text-right">Mva</th>
                </tr>
              </thead>
              <tbody>
                {#each report.lines as l (l.code + "-" + l.rate_bp)}
                  {@const isOpen = open === l.code}
                  {@const rows = postings[l.code]}
                  <tr>
                    <td>
                      <button
                        class="text-left link link-hover"
                        aria-expanded={isOpen}
                        aria-controls={"poster-" + l.code}
                        onclick={() => toggle(l.code)}
                      >
                        <span class="font-mono font-medium">{l.code}</span>
                        {l.description}{l.rate_bp ? ", " + String(l.rate_bp / 100).replace(".", ",") + " %" : ""}
                      </button>
                      {#if NOTE[l.retning]}<span class="block text-sm opacity-70">{NOTE[l.retning]}</span>{/if}
                      <span class="block text-sm opacity-70 sm:hidden">Grunnlag {kr(Math.abs(l.grunnlag_ore))}</span>
                    </td>
                    <td class="text-right tabular-nums whitespace-nowrap hidden sm:table-cell">{kr(Math.abs(l.grunnlag_ore))}</td>
                    <td class="text-right tabular-nums whitespace-nowrap">{kr(l.virkning_ore)}</td>
                  </tr>
                  {#if isOpen}
                    <tr id={"poster-" + l.code}>
                      <td colspan="3" class="bg-base-200/50">
                        {#if !rows}
                          <span class="loading loading-dots loading-sm"></span>
                        {:else if rows.error}
                          <span class="text-error">{rows.error}</span>
                        {:else}
                          <ul class="grid gap-1 text-sm">
                            {#each rows.postings as p, i (i)}
                              <li class="flex flex-wrap gap-x-3">
                                <a class="link font-medium" href={"#/c/" + companyId + "/hovedbok/" + p.voucher_id}>Bilag {p.voucher}</a>
                                <span class="opacity-70 whitespace-nowrap">{formatDate(p.date)}</span>
                                <span class="mr-auto min-w-0 break-words">
                                  {p.account} {p.account_name} · {p.description}{p.party_no ? " · part " + p.party_no : ""}
                                </span>
                                <span class="tabular-nums whitespace-nowrap">grunnlag {kr(Math.abs(p.grunnlag_ore))}</span>
                              </li>
                            {/each}
                          </ul>
                          {#if rows.total > rows.postings.length}
                            <p class="text-sm opacity-70 mt-1">Viser de første {rows.postings.length} av {rows.total}.</p>
                          {/if}
                        {/if}
                      </td>
                    </tr>
                  {/if}
                {/each}
              </tbody>
            </table>
          </div>
        {/if}

        <dl class="grid gap-x-6 gap-y-1 sm:grid-cols-3 tabular-nums border-t border-base-300 pt-3">
          <div>
            <dt class="text-sm opacity-70">Utgående mva</dt>
            <dd class="text-lg font-semibold">{kr(report.utgaende_ore)} kr</dd>
          </div>
          <div>
            <dt class="text-sm opacity-70">Fradrag</dt>
            <dd class="text-lg font-semibold">{kr(-report.inngaende_ore)} kr</dd>
          </div>
          <div>
            <dt class="text-sm opacity-70">{report.netto_ore >= 0 ? "Å betale" : "Til gode"}</dt>
            <dd class="text-lg font-semibold">{kr(Math.abs(report.netto_ore))} kr</dd>
          </div>
        </dl>
      </div>
    </section>

    <section class="card border border-base-300 bg-base-100 mb-6" aria-labelledby="melding-tittel">
      <div class="card-body gap-2">
        <h2 id="melding-tittel" class="card-title">Mva-meldingen</h2>
        <p>
          Meldingen lages fra spesifikasjonen over og kontrolleres mot Skatteetatens skjema. Direkte innsending fra regnmed kommer
          når tilgangen til Skatteetaten er på plass. Til da kan du laste den ned for å kontrollere den, eller levere den gjennom
          et system som kan sende den.
        </p>
        <button
          class="btn w-fit"
          onclick={() =>
            download(
              base + "/reports/mva-melding?year=" + report.year + "&termin=" + report.termin,
              "mva-melding-" + report.year + "-termin" + report.termin + ".xml",
            )}
        >
          Last ned mva-meldingen (XML)
        </button>
        <p class="text-sm opacity-70">
          SAF-T for hele året ligger under <a class="link" href={"#/c/" + companyId + "/rapporter/saft"}>Rapporter</a>.
        </p>
      </div>
    </section>
  {/if}

  <details>
    <summary class="cursor-pointer font-medium">Terminordning</summary>
    <Terminordning
      {companyId}
      {scheme}
      canEdit={harRett(companyId, "MVA_ORDNING_ADMIN")}
      onDone={() => api(base + "/mva/terminordning").then((r) => (scheme = r)).catch(() => {})}
    />
  </details>
{/if}
