<script>
  // Payments: what we owe suppliers, picked into a payment list, approved
  // (by someone else when attestering is on — four eyes on money out),
  // sent to the bank as a pain.001 file, and recorded as paid. Each step
  // is one-way, so each asks first and says what happens.
  import { api, post } from "../../lib/api.js";
  import { kr, formatDate, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import { harRett } from "../../lib/me.svelte.js";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId + "/payments");

  let payable = $state(null);
  let runs = $state(null);
  let error = $state(null);
  let picked = $state([]);
  let executionDate = $state(today());
  let busy = $state(false);

  let canCreate = $derived(harRett(companyId, "BETALING_OPPRETT"));
  let canApprove = $derived(harRett(companyId, "BETALING_GODKJENN"));
  let canSettle = $derived(harRett(companyId, "BETALING_OPPGJOR"));

  function load() {
    api(base + "/payable")
      .then((r) => (payable = r.items))
      .catch((e) => (error = e.message));
    api(base + "/runs")
      .then((r) => (runs = r.runs))
      .catch((e) => (error = e.message));
  }
  $effect(load);

  const todayIso = today();
  let selectable = $derived((payable || []).filter((p) => !p.i_kjoring && p.bank_account));
  let pickedSum = $derived(
    (payable || []).filter((p) => picked.includes(p.entry_id)).reduce((s, p) => s + p.belop_ore, 0),
  );
  let activeRuns = $derived((runs || []).filter((r) => r.status === "utkast" || r.status === "godkjent"));
  let pastRuns = $derived((runs || []).filter((r) => r.status === "utbetalt" || r.status === "annullert"));

  function toggle(id) {
    picked = picked.includes(id) ? picked.filter((p) => p !== id) : [...picked, id];
  }
  const pickDue = () =>
    (picked = selectable.filter((p) => p.due_date && p.due_date <= todayIso).map((p) => p.entry_id));
  const pickAll = () => (picked = selectable.map((p) => p.entry_id));

  async function createRun() {
    if (!picked.length || busy) return;
    busy = true;
    try {
      await post(base + "/runs", {
        items: picked.map((entry_id) => ({ entry_id })),
        execution_date: executionDate || null,
      });
      toast("Betalingslisten er laget. Den må godkjennes før filen kan lastes ned.", true);
      picked = [];
      load();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  async function act(r, path, body, done) {
    busy = true;
    try {
      const out = await post(base + "/runs/" + r.run_id + "/" + path, body);
      toast(done(out), true);
      load();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  async function approve(r) {
    const ok = await bekreft(
      "Betale " + kr(r.sum_ore) + " kr fordelt på " + r.antall + (r.antall === 1 ? " post" : " poster") +
        " " + formatDate(r.execution_date) + ". Betalingsfilen lages nå og kan ikke endres etterpå.",
      { tittel: "Godkjenn betalingslisten?", ok: "Godkjenn" },
    );
    if (ok) act(r, "approve", {}, () => "Godkjent. Last ned betalingsfilen og last den opp i nettbanken.");
  }

  async function cancel(r) {
    const ok = await bekreft("Postene blir åpne igjen og kan legges i en ny liste.", {
      tittel: "Annuller betalingslisten?",
      ok: "Annuller listen",
      farlig: true,
    });
    if (ok) act(r, "cancel", {}, () => "Betalingslisten er annullert");
  }

  async function settle(r) {
    const answer = await skjema(
      "Registrer at " + kr(r.sum_ore) + " kr er betalt",
      [
        { navn: "dato", etikett: "Betalt dato", type: "date", standard: r.execution_date || today() },
        { navn: "konto", etikett: "Betalt fra bankkonto", standard: "1920", hjelp: "Kontonummer i kontoplanen" },
      ],
      {
        ok: "Bokfør betalingen",
        melding: "Gjør dette når banken har betalt. Leverandørpostene lukkes, og det kan ikke angres.",
      },
    );
    if (answer) act(r, "settle", { dato: answer.dato, bank_konto: answer.konto }, (o) => "Betalingen er bokført som bilag " + o.voucher);
  }

  const fileOf = (r) => download(base + "/runs/" + r.run_id + "/file", "betaling-" + r.execution_date + ".xml");

  const STATUS = {
    utkast: ["Venter på godkjenning", "badge-warning"],
    godkjent: ["Godkjent, klar for nettbanken", "badge-info"],
    utbetalt: ["Betalt", "badge-success"],
    annullert: ["Annullert", "badge-ghost"],
  };
</script>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if payable === null || runs === null}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else}
  {#if activeRuns.length}
    <section class="mb-6" aria-labelledby="aktive-tittel">
      <h2 id="aktive-tittel" class="text-lg font-semibold mb-2">Betalingslister i arbeid</h2>
      <ul class="grid gap-2">
        {#each activeRuns as r (r.run_id)}
          <li class="card border border-base-300 bg-base-100">
            <div class="card-body p-4 gap-2">
              <div class="flex flex-wrap items-baseline gap-x-4 gap-y-1">
                <span class="font-semibold tabular-nums">{kr(r.sum_ore)} kr</span>
                <span class="opacity-70 mr-auto">{r.antall} {r.antall === 1 ? "post" : "poster"}, betales {formatDate(r.execution_date)}</span>
                <span class="badge badge-soft {STATUS[r.status][1]}">{STATUS[r.status][0]}</span>
              </div>
              {#if r.status === "utkast"}
                <p class="text-sm opacity-70">
                  Laget av {r.created_by}. Med attestering på må en annen person godkjenne.
                </p>
                <div class="flex flex-wrap gap-2">
                  {#if canApprove}<button class="btn btn-sm btn-primary" disabled={busy} onclick={() => approve(r)}>Godkjenn</button>{/if}
                  {#if canCreate}<button class="btn btn-sm btn-ghost" disabled={busy} onclick={() => cancel(r)}>Annuller</button>{/if}
                </div>
              {:else}
                <p class="text-sm opacity-70">
                  Godkjent av {r.approved_by}. Last ned betalingsfilen og last den opp i nettbanken. Når banken har betalt, registrer
                  det her.
                </p>
                <div class="flex flex-wrap gap-2">
                  <button class="btn btn-sm btn-primary" onclick={() => fileOf(r)}>Last ned betalingsfil</button>
                  {#if canSettle}<button class="btn btn-sm" disabled={busy} onclick={() => settle(r)}>Registrer betalt</button>{/if}
                </div>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <section class="card border border-base-300 bg-base-100 mb-6" aria-labelledby="betal-tittel">
    <div class="card-body overflow-x-auto">
      <h2 id="betal-tittel" class="card-title">Å betale</h2>
      {#if payable.length === 0}
        <p class="opacity-70">Ingen åpne leverandørposter. Leverandørfakturaer kommer hit når de er bokført fra Bilag.</p>
      {:else}
        {#if canCreate && selectable.length}
          <div class="flex flex-wrap items-end gap-2">
            <button class="btn btn-ghost btn-sm" onclick={pickDue}>Velg forfalte</button>
            <button class="btn btn-ghost btn-sm" onclick={pickAll}>Velg alle</button>
            <label class="fieldset py-0 ml-auto">
              <span class="fieldset-legend pt-0">Betalingsdato</span>
              <input type="date" class="input input-sm" min={todayIso} bind:value={executionDate} />
            </label>
            <button class="btn btn-primary btn-sm" disabled={!picked.length || busy} onclick={createRun}>
              {#if busy}<span class="loading loading-spinner loading-xs"></span>{/if}
              Lag betalingsliste{picked.length ? " (" + picked.length + ", " + kr(pickedSum) + " kr)" : ""}
            </button>
          </div>
        {/if}
        <table class="table table-sm sm:table-md mt-2">
          <caption class="sr-only">Åpne leverandørposter som kan betales, med forfall og KID.</caption>
          <thead>
            <tr>
              {#if canCreate}<th scope="col" class="w-px px-1 sm:px-4"><span class="sr-only">Velg</span></th>{/if}
              <th scope="col">Leverandør</th>
              <th scope="col" class="hidden sm:table-cell">Forfall</th>
              <th scope="col" class="hidden md:table-cell">KID</th>
              <th scope="col" class="text-right">Beløp</th>
            </tr>
          </thead>
          <tbody>
            {#each payable as p (p.entry_id)}
              {@const late = p.due_date && p.due_date < todayIso}
              <tr>
                {#if canCreate}
                  <td class="px-1 sm:px-4">
                    {#if p.i_kjoring}
                      <span class="badge badge-ghost badge-xs whitespace-nowrap">i liste</span>
                    {:else if !p.bank_account}
                      <span class="badge badge-warning badge-xs whitespace-nowrap">mangler konto</span>
                    {:else}
                      <input
                        type="checkbox"
                        class="checkbox checkbox-sm"
                        aria-label={"Velg " + p.party_name + ", bilag " + p.voucher}
                        checked={picked.includes(p.entry_id)}
                        onchange={() => toggle(p.entry_id)}
                      />
                    {/if}
                  </td>
                {/if}
                <td>
                  {p.party_name}
                  <span class="block text-sm opacity-70">Bilag {p.voucher}{p.description ? ", " + p.description : ""}</span>
                  <span class="block text-sm sm:hidden {late ? 'text-error' : ''}">
                    {p.due_date ? "Forfall " + formatDate(p.due_date) : "Uten forfall"}{p.kid ? ", KID " + p.kid : ""}
                  </span>
                  {#if !p.bank_account}
                    <a class="link text-sm" href={"#/c/" + companyId + "/leverandorer"}>Legg inn kontonummer under Leverandører</a>
                  {/if}
                </td>
                <td class="hidden sm:table-cell whitespace-nowrap {late ? 'text-error' : ''}">
                  {p.due_date ? formatDate(p.due_date) : "–"}
                  {#if p.kid}<span class="block text-xs opacity-70 md:hidden">KID {p.kid}</span>{/if}
                </td>
                <td class="hidden md:table-cell font-mono">{p.kid || "–"}</td>
                <td class="text-right tabular-nums whitespace-nowrap">{kr(p.belop_ore)} kr</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  </section>

  {#if pastRuns.length}
    <details>
      <summary class="cursor-pointer font-medium">Tidligere betalingslister ({pastRuns.length})</summary>
      <ul class="grid gap-1 text-sm mt-2">
        {#each pastRuns as r (r.run_id)}
          <li class="flex flex-wrap items-center gap-x-3">
            <span class="whitespace-nowrap opacity-70">{formatDate(r.execution_date)}</span>
            <span class="mr-auto">
              {r.antall} {r.antall === 1 ? "post" : "poster"}, {kr(r.sum_ore)} kr — {STATUS[r.status][0].toLowerCase()}{r.settled_voucher
                ? ", bilag " + r.settled_voucher
                : ""}
            </span>
            {#if r.status === "utbetalt"}
              <button class="btn btn-ghost btn-xs" onclick={() => fileOf(r)}>Betalingsfil</button>
            {/if}
          </li>
        {/each}
      </ul>
    </details>
  {/if}
{/if}
