<script>
  // Leverandørsiden: hva vi skylder, hva som forfaller når, og ett trykk
  // fra åpne poster til betalingslisten (som godkjennes under Bank — fire
  // øyne). Forfall og KID kommer fra leverandørens faktura når den
  // bokføres fra innboksen, og kan føres eller rettes her (docs/betaling.md).
  import { api, post } from "../../lib/api.js";
  import { kr, formatDate, today as todayIso } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { skjema } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import { harRett } from "../../lib/me.svelte.js";
  import PartyContact from "../../components/PartyContact.svelte";
  import Part from "../reskontro/Part.svelte";

  let { companyId, partyId } = $props();

  let party = $state(null);
  let items = $state(null);
  let inRun = $state(new Set());
  let error = $state(null);
  let picked = $state([]);
  let busy = $state(false);

  const base = $derived("/companies/" + companyId);

  function load() {
    api(base + "/parties/" + partyId)
      .then((p) => (party = p))
      .catch((e) => (error = e.message));
    api(base + "/parties/" + partyId + "/items?open=true")
      // What we owe: open credit posts.
      .then((r) => (items = r.items.filter((i) => i.remaining_ore < 0)))
      .catch((e) => (error = e.message));
    api(base + "/payments/payable")
      .then((r) => (inRun = new Set(r.items.filter((i) => i.i_kjoring).map((i) => i.entry_id))))
      .catch(() => (inRun = new Set()));
  }
  $effect(load);

  const today = todayIso();
  let payable = $derived((items || []).filter((i) => !inRun.has(i.entry_id)));
  let pickedSum = $derived(
    (items || []).filter((i) => picked.includes(i.entry_id)).reduce((s, i) => s - i.remaining_ore, 0),
  );
  let canPay = $derived(harRett(companyId, "BETALING_OPPRETT"));
  let canEdit = $derived(harRett(companyId, "RESKONTRO_SKRIV"));
  let canEditContact = $derived(harRett(companyId, "KONTAKT_SKRIV"));

  function toggle(id) {
    picked = picked.includes(id) ? picked.filter((p) => p !== id) : [...picked, id];
  }
  function pickDue() {
    picked = payable.filter((i) => i.due_date && i.due_date <= today).map((i) => i.entry_id);
  }

  async function addToPaymentList() {
    busy = true;
    try {
      await post(base + "/payments/runs", { items: picked.map((entry_id) => ({ entry_id })) });
      toast(picked.length + (picked.length === 1 ? " post" : " poster") + " lagt i en betalingsliste", true);
      picked = [];
      load();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  async function editTerms(item) {
    const answer = await skjema(
      "Forfall og KID for bilag " + item.voucher,
      [
        { navn: "due_date", etikett: "Forfall", type: "date", standard: item.due_date || "", valgfri: true },
        { navn: "kid", etikett: "KID", standard: item.kid || "", valgfri: true },
      ],
      { ok: "Lagre" },
    );
    if (!answer) return;
    try {
      await post(base + "/reskontro/entries/" + item.entry_id + "/terms", {
        due_date: answer.due_date || null,
        kid: answer.kid?.trim() || null,
      });
      toast("Forfall og KID er lagret", true);
      load();
    } catch (e) {
      toast(e.message, false);
    }
  }

  async function showVoucher(item) {
    try {
      const r = await api(base + "/vouchers/" + item.voucher_id + "/attachments");
      const first = r.attachments?.[0];
      if (!first) return toast("Bilaget har ikke noe vedlegg", false);
      download(base + "/attachments/" + first.attachment_id, first.filename);
    } catch (e) {
      toast(e.message, false);
    }
  }
</script>

<nav class="mb-3">
  <a class="link link-hover text-sm" href={"#/c/" + companyId + "/leverandorer"}>← Alle leverandører</a>
</nav>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if !party}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else if party.kind !== "leverandor"}
  <div class="alert alert-info">
    <span>{party.name} er en kunde, ikke en leverandør.</span>
    <a class="btn btn-sm" href={"#/c/" + companyId + "/kunder/" + party.party_id}>Åpne under Kunder</a>
  </div>
{:else}
  <h1 class="text-2xl font-bold">{party.name}</h1>
  <p class="opacity-70 mb-4">
    Leverandør nr. {party.party_no}{party.orgnr ? ", orgnr " + party.orgnr : ""}
  </p>

  {#if !party.bank_account}
    <div class="alert alert-warning alert-soft mb-4">
      <span>Leverandøren har ikke kontonummer, så postene kan ikke betales fra betalingslisten.</span>
      {#if canEditContact}
        <button class="btn btn-sm" onclick={() => document.querySelector("#kontakt input[inputmode=numeric]")?.focus()}>
          Legg inn kontonummer
        </button>
      {/if}
    </div>
  {/if}

  <div class="stats stats-vertical sm:stats-horizontal border border-base-300 bg-base-100 w-full mb-6">
    <div class="stat">
      <div class="stat-title">Skyldig</div>
      <div class="stat-value text-2xl tabular-nums">{kr(-party.saldo_ore)}</div>
    </div>
    <div class="stat">
      <div class="stat-title">Forfalt</div>
      <div class="stat-value text-2xl tabular-nums {party.overdue_ore ? 'text-error' : ''}">
        {kr(party.overdue_ore ?? 0)}
      </div>
    </div>
  </div>

  <section class="card border border-base-300 bg-base-100 mb-6" aria-labelledby="apne-tittel">
    <div class="card-body overflow-x-auto">
      <h2 id="apne-tittel" class="card-title">Åpne poster</h2>
      {#if items === null}
        <span class="loading loading-spinner"></span>
      {:else if items.length === 0}
        <p class="opacity-70">Dere skylder ikke leverandøren noe.</p>
      {:else}
        {#if canPay && party.bank_account && payable.length}
          <div class="flex flex-wrap items-center gap-2">
            <button class="btn btn-primary btn-sm" disabled={!picked.length || busy} onclick={addToPaymentList}>
              {#if busy}<span class="loading loading-spinner loading-xs"></span>{/if}
              Legg i betalingsliste{picked.length ? " (" + kr(pickedSum) + ")" : ""}
            </button>
            <button class="btn btn-ghost btn-sm" onclick={pickDue}>Velg forfalte</button>
            <span class="text-sm opacity-70">
              Betalingslisten godkjennes av en annen under
              <a class="link" href={"#/c/" + companyId + "/bank"}>Bank</a>.
            </span>
          </div>
        {/if}
        <table class="table table-sm sm:table-md mt-2">
          <caption class="sr-only">Åpne poster hos leverandøren, med forfall og KID.</caption>
          <thead>
            <tr>
              {#if canPay && party.bank_account}<th scope="col" class="w-px px-1 sm:px-4"><span class="sr-only">Velg</span></th>{/if}
              <th scope="col">Bilag</th>
              <th scope="col" class="hidden sm:table-cell">Forfall</th>
              <th scope="col" class="hidden md:table-cell">KID</th>
              <th scope="col" class="text-right">Skyldig</th>
              <th scope="col" class="hidden sm:table-cell"><span class="sr-only">Handlinger</span></th>
            </tr>
          </thead>
          <tbody>
            {#each items as i (i.entry_id)}
              <tr>
                {#if canPay && party.bank_account}
                  <td class="px-1 sm:px-4">
                    {#if inRun.has(i.entry_id)}
                      <span class="badge badge-ghost badge-xs whitespace-nowrap" title="Ligger i en betalingsliste">i liste</span>
                    {:else}
                      <input
                        type="checkbox"
                        class="checkbox checkbox-sm"
                        aria-label={"Velg bilag " + i.voucher}
                        checked={picked.includes(i.entry_id)}
                        onchange={() => toggle(i.entry_id)}
                      />
                    {/if}
                  </td>
                {/if}
                <td>
                  {i.voucher}
                  <span class="block text-sm opacity-70">{i.description || formatDate(i.date)}</span>
                  <span class="block text-sm sm:hidden {i.due_date && i.due_date < today ? 'text-error' : ''}">
                    {i.due_date ? "Forfall " + formatDate(i.due_date) : "Uten forfall"}{i.kid ? ", KID " + i.kid : ""}
                  </span>
                  <!-- On a phone the actions sit under the post instead of in a column. -->
                  <span class="flex gap-1 mt-1 sm:hidden">
                    <button class="btn btn-ghost btn-xs" onclick={() => showVoucher(i)}>Bilag</button>
                    {#if canEdit}
                      <button class="btn btn-ghost btn-xs" onclick={() => editTerms(i)} aria-label={"Forfall og KID for bilag " + i.voucher}>
                        Forfall/KID
                      </button>
                    {/if}
                  </span>
                </td>
                <td class="hidden sm:table-cell whitespace-nowrap {i.due_date && i.due_date < today ? 'text-error' : ''}">
                  {i.due_date ? formatDate(i.due_date) : "–"}
                  {#if i.kid}<span class="block text-xs opacity-70 md:hidden">KID {i.kid}</span>{/if}
                </td>
                <td class="hidden md:table-cell font-mono">{i.kid || "–"}</td>
                <td class="text-right tabular-nums whitespace-nowrap">{kr(-i.remaining_ore)}</td>
                <td class="whitespace-nowrap text-right hidden sm:table-cell">
                  <button class="btn btn-ghost btn-xs" onclick={() => showVoucher(i)}>Bilag</button>
                  {#if canEdit}
                    <button class="btn btn-ghost btn-xs" onclick={() => editTerms(i)} aria-label={"Forfall og KID for bilag " + i.voucher}>
                      Forfall/KID
                    </button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  </section>

  <section id="kontakt" class="card border border-base-300 bg-base-100 max-w-xl mb-6" aria-labelledby="kontakt-tittel">
    <div class="card-body">
      <h2 id="kontakt-tittel" class="card-title">Kontaktinfo</h2>
      {#if canEditContact}
        {#key party.party_id}<PartyContact {companyId} {party} onSaved={load} />{/key}
      {:else}
        <dl class="grid gap-2 text-sm">
          <dt class="opacity-70">Kontonummer</dt><dd>{party.bank_account || "–"}</dd>
          <dt class="opacity-70">Adresse</dt><dd>{party.address || "–"}</dd>
          <dt class="opacity-70">E-post</dt><dd>{party.email || "–"}</dd>
        </dl>
      {/if}
    </div>
  </section>

  <details>
    <summary class="cursor-pointer font-medium mb-3">Alle poster i reskontroen</summary>
    <Part {companyId} {party} partyId={party.party_id} contact={false} embedded />
  </details>
{/if}
