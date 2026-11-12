<script>
  // Reconciliation: what the bank shows that the books do not. Each bank
  // line without a match offers what can close it — the posting with
  // exactly its amount (never another amount), posting it right here
  // when nothing is booked yet (a fee, interest), or a search. The
  // comparison is made on the statement's own end date, so the
  // difference is real and not just "everything posted since".
  import { api, post, send } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";
  import KontoVelger from "../../components/KontoVelger.svelte";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId);

  let accounts = $state(null);
  let account = $state("");
  let recon = $state(null);
  let error = $state(null);
  let chart = $state({ kontoer: [], standard: [] });
  let uploading = $state(false);
  let dragging = $state(false);
  // One open helper per bank line: { id, mode: "post" | "search" }.
  let open = $state(null);
  let busyId = $state(null);
  let postAccount = $state("");
  let postText = $state("");
  let query = $state("");
  let found = $state(null);

  let canReconcile = $derived(harRett(companyId, "BANK_AVSTEM"));
  let canPost = $derived(canReconcile && harRett(companyId, "BILAG_BOKFOR"));

  $effect(() => {
    api(base + "/bank/accounts")
      .then((r) => {
        accounts = r.accounts;
        const pick =
          r.accounts.find((a) => a.unmatched_bank > 0) ||
          r.accounts.find((a) => a.last_statement_to) ||
          r.accounts.find((a) => a.number === "1920") ||
          r.accounts[0];
        account = pick?.number || "";
      })
      .catch((e) => (error = e.message));
    if (canPost) {
      api(base + "/accounts")
        .then((a) => (chart = { kontoer: a.kontoer || [], standard: a.standard || [] }))
        .catch(() => {});
    }
  });

  function load() {
    if (!account) return;
    const asked = account;
    api(base + "/bank/reconciliation?account=" + asked)
      .then((r) => asked === account && (recon = r))
      .catch((e) => (error = e.message));
  }
  $effect(() => {
    recon = null;
    open = null;
    load();
  });

  async function upload(file) {
    if (!file || uploading) return;
    uploading = true;
    try {
      const r = await api(base + "/bank/statements?account=" + account, { method: "POST", body: await file.text() });
      toast(
        r.transactions + " bankposter lest inn, " + r.auto_matched + " koblet automatisk",
        true,
      );
      refreshAccounts();
      load();
    } catch (e) {
      toast(e.message, false);
    }
    uploading = false;
  }

  function refreshAccounts() {
    api(base + "/bank/accounts").then((r) => (accounts = r.accounts)).catch(() => {});
  }

  async function match(line, entry) {
    busyId = line.bank_transaction_id;
    try {
      await post(base + "/bank/matches", { bank_transaction_id: line.bank_transaction_id, entry_id: entry.entry_id });
      toast("Koblet til bilag " + entry.voucher, true);
      open = null;
      refreshAccounts();
      load();
    } catch (e) {
      toast(e.message, false);
    }
    busyId = null;
  }

  function toggle(line, mode) {
    if (open?.id === line.bank_transaction_id && open.mode === mode) return (open = null);
    open = { id: line.bank_transaction_id, mode };
    if (mode === "post") {
      // Money out without a voucher is most often a bank fee, money in
      // interest — the standard chart's 7700 "Andre kostnader" and 8050
      // "Annen renteinntekt". Suggestions only; the field is the user's.
      postAccount = line.amount_ore < 0 ? "7700" : "8050";
      postText = line.description || "";
    } else {
      query = kr(Math.abs(line.amount_ore));
      search();
    }
  }

  async function postLine(event, line) {
    event.preventDefault();
    const nr = postAccount.trim();
    if (!nr || busyId) return;
    busyId = line.bank_transaction_id;
    try {
      if (!chart.kontoer.some((k) => k.number === nr) && chart.standard.some((s) => s.number === nr)) {
        await post(base + "/accounts", { number: nr });
      }
      const r = await post(base + "/bank/transactions/" + line.bank_transaction_id + "/post", {
        konto: nr,
        tekst: postText.trim(),
      });
      toast("Bokført som bilag " + r.voucher + " og koblet", true);
      open = null;
      refreshAccounts();
      load();
    } catch (e) {
      toast(e.message, false);
    }
    busyId = null;
  }

  let searchFor = "";
  function search(event) {
    event?.preventDefault();
    const q = query.trim();
    searchFor = q;
    found = null;
    api(base + "/bank/entries?account=" + account + "&q=" + encodeURIComponent(q))
      .then((r) => searchFor === q && (found = r.entries))
      .catch((e) => toast(e.message, false));
  }

  async function unmatch(m) {
    const ok = await bekreft(
      "Koblingen mellom bankposten " + formatDate(m.booking_date) + " (" + kr(m.amount_ore) + " kr) og bilag " + m.voucher +
        " fjernes. Bilaget står urørt; bankposten blir å avstemme igjen.",
      { tittel: "Fjern koblingen?", ok: "Fjern kobling" },
    );
    if (!ok) return;
    try {
      await send("DELETE", base + "/bank/matches/" + m.bank_transaction_id);
      toast("Koblingen er fjernet", true);
      refreshAccounts();
      load();
    } catch (e) {
      toast(e.message, false);
    }
  }

  let difference = $derived(
    recon && recon.statement_closing_ore != null && recon.ledger_at_statement_ore != null
      ? recon.statement_closing_ore - recon.ledger_at_statement_ore
      : null,
  );
  let selected = $derived(accounts?.find((a) => a.number === account));
  const accountName = (nr) =>
    [...chart.kontoer, ...chart.standard].find((k) => k.number === nr.trim())?.name;
</script>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if accounts === null}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else if accounts.length === 0}
  <div class="alert alert-info" role="status">
    <span>Selskapet har ingen bankkonto (19xx) i kontoplanen ennå. Legg den til under Hovedbok.</span>
  </div>
{:else}
  <div class="flex flex-wrap items-end gap-3 mb-4">
    {#if accounts.length > 1}
      <label class="fieldset py-0">
        <span class="fieldset-legend pt-0">Bankkonto</span>
        <select class="select" bind:value={account}>
          {#each accounts as a (a.number)}
            <option value={a.number}>{a.number} {a.name}{a.unmatched_bank ? " (" + a.unmatched_bank + " å avstemme)" : ""}</option>
          {/each}
        </select>
      </label>
    {:else}
      <p class="font-medium">{selected?.number} {selected?.name}</p>
    {/if}
    {#if canReconcile}
      <label class="btn btn-primary ml-auto {uploading ? 'btn-disabled' : ''}">
        {#if uploading}<span class="loading loading-spinner loading-sm"></span>{/if}
        Last opp kontoutskrift
        <input
          type="file"
          class="sr-only"
          accept=".xml,.csv,.txt"
          onchange={(e) => {
            upload(e.target.files[0]);
            e.target.value = "";
          }}
        />
      </label>
    {/if}
  </div>

  <div
    class="rounded-box {dragging ? 'outline-2 outline-dashed outline-primary' : ''}"
    role="region"
    aria-label="Avstemming"
    ondragover={(e) => {
      if (!canReconcile) return;
      e.preventDefault();
      dragging = true;
    }}
    ondragleave={() => (dragging = false)}
    ondrop={(e) => {
      e.preventDefault();
      dragging = false;
      if (canReconcile) upload(e.dataTransfer.files[0]);
    }}
  >
    {#if recon === null}
      <div class="flex justify-center p-12"><span class="loading loading-spinner"></span></div>
    {:else if recon.statements.length === 0}
      <div class="card border border-base-300 bg-base-100">
        <div class="card-body">
          <h2 class="card-title">Ingen kontoutskrift ennå</h2>
          <p>
            Last ned kontoutskriften fra nettbanken som camt.053 (XML) eller CSV, og last den opp her — eller dra filen
            inn. Bankpostene kobles automatisk mot bilagene med samme beløp og dato.
          </p>
        </div>
      </div>
    {:else}
      <section class="card border border-base-300 bg-base-100 mb-6" aria-labelledby="status-tittel">
        <div class="card-body gap-3">
          <h2 id="status-tittel" class="card-title">
            {recon.unmatched_bank.length === 0
              ? "Alt i banken er avstemt"
              : recon.unmatched_bank.length === 1
                ? "1 bankpost å avstemme"
                : recon.unmatched_bank.length + " bankposter å avstemme"}
          </h2>
          <dl class="grid gap-x-6 gap-y-1 sm:grid-cols-3 tabular-nums">
            <div>
              <dt class="text-sm opacity-70">Banken {formatDate(recon.statement_to_date)}</dt>
              <dd class="text-lg font-semibold">
                {recon.statement_closing_ore != null ? kr(recon.statement_closing_ore) + " kr" : "ikke oppgitt"}
              </dd>
            </div>
            <div>
              <dt class="text-sm opacity-70">Hovedboken samme dato</dt>
              <dd class="text-lg font-semibold">{kr(recon.ledger_at_statement_ore ?? recon.ledger_balance_ore)} kr</dd>
            </div>
            <div>
              <dt class="text-sm opacity-70">Differanse</dt>
              <dd class="text-lg font-semibold {difference ? 'text-warning' : ''}">
                {difference == null ? "–" : kr(difference) + " kr"}
              </dd>
            </div>
          </dl>
          {#if recon.statement_closing_ore == null}
            <p class="text-sm opacity-70">CSV-filer fra nettbanken har ikke saldo, så differansen kan ikke regnes. camt.053 har den.</p>
          {/if}
        </div>
      </section>

      {#if recon.unmatched_bank.length}
        <section class="mb-6" aria-labelledby="ukoblet-tittel">
          <h2 id="ukoblet-tittel" class="text-lg font-semibold mb-2">I banken, ikke i regnskapet</h2>
          <ul class="grid gap-2">
            {#each recon.unmatched_bank as line (line.bank_transaction_id)}
              {@const isOpen = open?.id === line.bank_transaction_id}
              <li class="card border border-base-300 bg-base-100">
                <div class="card-body p-4 gap-3">
                  <div class="flex flex-wrap items-baseline gap-x-4 gap-y-1">
                    <span class="whitespace-nowrap opacity-70">{formatDate(line.booking_date)}</span>
                    <span class="font-medium break-words mr-auto">{line.description || "Uten tekst"}</span>
                    <span class="tabular-nums font-semibold whitespace-nowrap {line.amount_ore < 0 ? '' : 'text-success'}">
                      {kr(line.amount_ore)} kr
                    </span>
                  </div>
                  {#if canReconcile}
                    <div class="flex flex-wrap gap-2">
                      {#each line.suggestions as s, i (s.entry_id)}
                        <button
                          class="btn btn-sm {i === 0 ? 'btn-primary' : ''}"
                          disabled={busyId === line.bank_transaction_id}
                          onclick={() => match(line, s)}
                        >
                          Koble til bilag {s.voucher} ({formatDate(s.date)}{s.description ? ", " + s.description : ""})
                        </button>
                      {/each}
                      {#if canPost}
                        <button
                          class="btn btn-sm {line.suggestions.length ? 'btn-ghost' : 'btn-primary'}"
                          aria-expanded={isOpen && open.mode === "post"}
                          onclick={() => toggle(line, "post")}
                        >
                          Bokfør
                        </button>
                      {/if}
                      <button class="btn btn-sm btn-ghost" aria-expanded={isOpen && open.mode === "search"} onclick={() => toggle(line, "search")}>
                        Finn bilag
                      </button>
                    </div>
                  {/if}

                  {#if isOpen && open.mode === "post"}
                    <form class="grid gap-3 sm:grid-cols-[10rem_1fr_auto] items-end" onsubmit={(e) => postLine(e, line)}>
                      <div class="fieldset py-0">
                        <label class="fieldset-legend" for={"konto-" + line.bank_transaction_id}>Konto</label>
                        <KontoVelger
                          id={"konto-" + line.bank_transaction_id}
                          kontoer={chart.kontoer}
                          standard={chart.standard}
                          bind:value={postAccount}
                          cls="input w-full"
                        />
                      </div>
                      <div class="fieldset py-0">
                        <label class="fieldset-legend" for={"tekst-" + line.bank_transaction_id}>Tekst på bilaget</label>
                        <input id={"tekst-" + line.bank_transaction_id} class="input w-full" bind:value={postText} />
                      </div>
                      <button class="btn btn-primary" type="submit" disabled={!postAccount.trim() || busyId === line.bank_transaction_id}>
                        {#if busyId === line.bank_transaction_id}<span class="loading loading-spinner loading-sm"></span>{/if}
                        Bokfør og koble
                      </button>
                      <p class="text-sm opacity-70 sm:col-span-3">
                        Bokføres {formatDate(line.booking_date)}: {kr(Math.abs(line.amount_ore))} kr {line.amount_ore < 0 ? "ut av" : "inn på"}
                        {account} mot {postAccount.trim() || "…"}{accountName(postAccount) ? " " + accountName(postAccount) : ""}, uten mva.
                        Har utgiften mva eller en kvittering, legg kvitteringen i Bilag i stedet.
                      </p>
                    </form>
                  {:else if isOpen && open.mode === "search"}
                    <div class="grid gap-2">
                      <form class="join w-full max-w-md" role="search" onsubmit={search}>
                        <label class="sr-only" for={"sok-" + line.bank_transaction_id}>Søk etter bilag</label>
                        <input
                          id={"sok-" + line.bank_transaction_id}
                          class="input join-item w-full"
                          placeholder="Beløp, tekst eller bilagsnummer"
                          bind:value={query}
                        />
                        <button class="btn join-item" type="submit">Søk</button>
                      </form>
                      {#if found === null}
                        <span class="loading loading-dots loading-sm"></span>
                      {:else if found.length === 0}
                        <p class="text-sm opacity-70">Ingen åpne bilag på bankkontoen passer. Er det ikke bokført, bruk «Bokfør».</p>
                      {:else}
                        <ul class="grid gap-1">
                          {#each found as f (f.entry_id)}
                            {@const same = f.amount_ore === line.amount_ore}
                            <li class="flex flex-wrap items-center gap-2 text-sm">
                              <span class="tabular-nums w-24">{kr(f.amount_ore)} kr</span>
                              <span class="mr-auto">Bilag {f.voucher}, {formatDate(f.date)}{f.description ? ", " + f.description : ""}</span>
                              {#if same}
                                <button class="btn btn-xs" onclick={() => match(line, f)}>Koble</button>
                              {:else}
                                <span class="opacity-70">annet beløp</span>
                              {/if}
                            </li>
                          {/each}
                        </ul>
                      {/if}
                    </div>
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if recon.unmatched_entries.length}
        <details class="mb-4">
          <summary class="cursor-pointer font-medium">
            I regnskapet, ikke i banken ennå ({recon.unmatched_entries.length})
          </summary>
          <p class="text-sm opacity-70 my-2">Bokført på bankkontoen i perioden kontoutskriftene dekker, uten en bankpost. Ofte betalinger som ikke er trukket ennå.</p>
          <ul class="grid gap-1 text-sm">
            {#each recon.unmatched_entries as e (e.entry_id)}
              <li class="flex flex-wrap gap-x-3">
                <span class="opacity-70 whitespace-nowrap">{formatDate(e.date)}</span>
                <span class="mr-auto">Bilag {e.voucher}{e.description ? ", " + e.description : ""}</span>
                <span class="tabular-nums whitespace-nowrap">{kr(e.amount_ore)} kr</span>
              </li>
            {/each}
          </ul>
        </details>
      {/if}

      {#if recon.recent_matches.length}
        <details class="mb-4">
          <summary class="cursor-pointer font-medium">Koblet ({recon.matched_count})</summary>
          <ul class="grid gap-1 text-sm mt-2">
            {#each recon.recent_matches as m (m.bank_transaction_id)}
              <li class="flex flex-wrap items-center gap-x-3">
                <span class="opacity-70 whitespace-nowrap">{formatDate(m.booking_date)}</span>
                <span class="mr-auto">{m.description || "Uten tekst"} → bilag {m.voucher}{m.method === "auto" ? " (automatisk)" : ""}</span>
                <span class="tabular-nums whitespace-nowrap">{kr(m.amount_ore)} kr</span>
                {#if canReconcile}
                  <button class="btn btn-ghost btn-xs" onclick={() => unmatch(m)}>Fjern kobling</button>
                {/if}
              </li>
            {/each}
          </ul>
          {#if recon.matched_count > recon.recent_matches.length}
            <p class="text-sm opacity-70 mt-1">Viser de {recon.recent_matches.length} nyeste.</p>
          {/if}
        </details>
      {/if}

      <details>
        <summary class="cursor-pointer font-medium">Kontoutskrifter ({recon.statements.length})</summary>
        <ul class="grid gap-1 text-sm mt-2">
          {#each recon.statements as s (s.statement_id)}
            <li class="flex flex-wrap gap-x-3">
              <span class="mr-auto">
                {s.from_date ? formatDate(s.from_date) + "–" + formatDate(s.to_date) : "Uten periode"}, {s.transactions} poster
              </span>
              <span class="opacity-70">lastet opp {formatDate(s.imported_at)} av {s.imported_by}</span>
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  </div>
{/if}
