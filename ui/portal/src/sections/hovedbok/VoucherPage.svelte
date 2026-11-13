<script>
  // One voucher: its lines, its documents, and its corrections. A posted
  // voucher is never edited (bokføringsloven § 9): it is reversed — a new
  // voucher with every line mirrored, linked to this one — and the right
  // one posted after. A voucher owned by another flow (an invoice, a
  // payroll run, …) says where it is corrected instead.
  import { api, post } from "../../lib/api.js";
  import { kr, formatDate, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { skjema } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import { harRett } from "../../lib/me.svelte.js";

  let { companyId, voucherId } = $props();

  const base = $derived("/companies/" + companyId);
  let v = $state(null);
  let docs = $state(null);
  let error = $state(null);
  let busy = $state(false);

  let canPost = $derived(harRett(companyId, "BILAG_BOKFOR"));
  let canAttach = $derived(harRett(companyId, "VEDLEGG_SKRIV"));

  function load() {
    api(base + "/vouchers/" + voucherId)
      .then((r) => (v = r))
      .catch((e) => (error = e.message));
    api(base + "/vouchers/" + voucherId + "/attachments")
      .then((r) => (docs = r.attachments))
      .catch(() => (docs = []));
  }
  $effect(load);

  const label = (x) => (x.journal && x.journal !== "GL" ? x.journal + " " : "") + x.voucher;
  let debit = $derived((v?.lines || []).reduce((s, l) => s + Math.max(l.amount_ore, 0), 0));
  let canReverse = $derived(canPost && v && !v.owner && !v.reverses && !v.reversed_by && v.journal !== "IMP");

  async function reverse() {
    const answer = await skjema(
      "Tilbakefør bilag " + label(v),
      [{ navn: "dato", etikett: "Dato for tilbakeføringen", type: "date", standard: today() }],
      {
        ok: "Tilbakefør",
        melding:
          "Et nytt bilag med alle linjene speilvendt bokføres og knyttes til dette. Bilag " + label(v) +
          " står urørt. Kunde- og leverandørposter avstemmes mot speilbildet. Datoen må ligge i en åpen periode.",
      },
    );
    if (!answer) return;
    busy = true;
    try {
      const r = await post(base + "/vouchers/" + voucherId + "/reverse", { dato: answer.dato });
      toast("Tilbakeført som bilag " + r.voucher + ". Før det riktige bilaget under Nytt bilag.", true);
      location.hash = "#/c/" + companyId + "/hovedbok/" + r.voucher_id;
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  async function attach(event) {
    const file = event.currentTarget.files[0];
    event.currentTarget.value = "";
    if (!file || busy) return;
    busy = true;
    try {
      await api(base + "/vouchers/" + voucherId + "/attachments?filename=" + encodeURIComponent(file.name), {
        method: "POST",
        headers: { "content-type": file.type || "application/octet-stream" },
        body: file,
      });
      toast(file.name + " er lagt ved", true);
      load();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  const sizeOf = (n) => (n > 1_000_000 ? (n / 1_000_000).toFixed(1).replace(".", ",") + " MB" : Math.max(1, Math.round(n / 1000)) + " kB");
</script>

<nav class="mb-3">
  <a class="link link-hover text-sm" href={"#/c/" + companyId + "/hovedbok"}>← Alle bilag</a>
</nav>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if !v}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else}
  <div class="flex flex-wrap items-start gap-3 mb-4">
    <div class="mr-auto min-w-0">
      <h1 class="text-2xl font-bold">Bilag {label(v)}</h1>
      <p class="opacity-70">{formatDate(v.date)} · {v.description}</p>
      <p class="text-sm opacity-70">Bokført av {v.created_by} {formatDate(v.created_at)}</p>
    </div>
    {#if canReverse}
      <button class="btn" disabled={busy} onclick={reverse}>Tilbakefør bilaget</button>
    {/if}
  </div>

  {#if v.reverses}
    <div class="alert alert-info alert-soft mb-4" role="status">
      <span>Dette bilaget tilbakefører <a class="link" href={"#/c/" + companyId + "/hovedbok/" + v.reverses.voucher_id}>bilag {v.reverses.voucher}</a>.</span>
    </div>
  {:else if v.reversed_by}
    <div class="alert alert-warning alert-soft mb-4" role="status">
      <span>Tilbakeført av <a class="link" href={"#/c/" + companyId + "/hovedbok/" + v.reversed_by.voucher_id}>bilag {v.reversed_by.voucher}</a>.</span>
    </div>
  {:else if v.owner}
    <div class="alert alert-soft mb-4" role="status">
      <span>Bilaget hører til {v.owner}.</span>
    </div>
  {/if}

  <section class="card border border-base-300 bg-base-100 mb-6" aria-labelledby="linjer-tittel">
    <div class="card-body overflow-x-auto">
      <h2 id="linjer-tittel" class="card-title">Linjer</h2>
      <table class="table table-sm">
        <caption class="sr-only">Linjene på bilag {label(v)}, debet og kredit.</caption>
        <thead>
          <tr>
            <th scope="col">Konto</th>
            <th scope="col" class="text-right">Debet</th>
            <th scope="col" class="text-right">Kredit</th>
          </tr>
        </thead>
        <tbody>
          {#each v.lines as l, i (i)}
            <tr>
              <td>
                <a class="link link-hover font-mono" href={"#/c/" + companyId + "/hovedbok/" + l.account + "?ar=" + v.date.slice(0, 4)}>{l.account}</a>
                {l.account_name}
                <span class="block text-sm opacity-70">
                  {[l.description, l.party_no && "part " + l.party_no, l.vat_code && "mva-kode " + l.vat_code, l.avdeling && "avdeling " + l.avdeling, l.prosjekt && "prosjekt " + l.prosjekt, l.valuta && l.valuta + " " + kr(l.valutabelop_cent)]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
              </td>
              <td class="text-right tabular-nums whitespace-nowrap">{l.amount_ore > 0 ? kr(l.amount_ore) : ""}</td>
              <td class="text-right tabular-nums whitespace-nowrap">{l.amount_ore < 0 ? kr(-l.amount_ore) : ""}</td>
            </tr>
          {/each}
        </tbody>
        <tfoot>
          <tr>
            <th scope="row">Sum</th>
            <td class="text-right tabular-nums">{kr(debit)}</td>
            <td class="text-right tabular-nums">{kr(debit)}</td>
          </tr>
        </tfoot>
      </table>
    </div>
  </section>

  <section class="card border border-base-300 bg-base-100" aria-labelledby="dok-tittel">
    <div class="card-body">
      <h2 id="dok-tittel" class="card-title">Dokumentasjon</h2>
      {#if docs === null}
        <span class="loading loading-spinner loading-sm"></span>
      {:else if docs.length === 0}
        <p class="{v.journal === 'IMP' || v.reverses ? 'opacity-70' : 'text-warning'}">
          {v.journal === "IMP"
            ? "Importert historikk: dokumentasjonen er kildefilen (Rapporter → Revisjon)."
            : v.reverses
              ? "En tilbakeføring dokumenteres av bilaget den tilbakefører, " + v.reverses.voucher + "."
              : "Bilaget har ingen vedlegg. Bokføringsloven krever dokumentasjon for hver postering."}
        </p>
      {:else}
        <ul class="grid gap-1">
          {#each docs as d (d.attachment_id)}
            <li class="flex flex-wrap items-center gap-x-3">
              <button class="link mr-auto break-all text-left" onclick={() => download(base + "/attachments/" + d.attachment_id, d.filename)}>
                {d.filename}
              </button>
              <span class="text-sm opacity-70">{sizeOf(d.byte_size)}, lagt ved av {d.uploaded_by}</span>
            </li>
          {/each}
        </ul>
      {/if}
      {#if canAttach && v.journal !== "IMP"}
        <div class="fieldset mt-2">
          <label class="fieldset-legend" for="vedlegg">Legg ved dokument</label>
          <input id="vedlegg" type="file" class="file-input w-full max-w-md" disabled={busy} onchange={attach} />
          <span class="label whitespace-normal">Vedlegg kan ikke endres eller fjernes etterpå (hashen kontrolleres).</span>
        </div>
      {/if}
    </div>
  </section>
{/if}
