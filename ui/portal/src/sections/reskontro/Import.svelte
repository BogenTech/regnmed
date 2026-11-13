<script>
  // Migration: contacts and open items from another system as CSV (#19,
  // docs/migration.md). Columns are read from the HEADERS; open items are
  // always previewed row by row before anything is posted, and they
  // REPLACE the lump sum on the reskontro account.
  import { api } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId + "/import/");

  let kind = $state("kunde");
  let busy = $state(false);
  let error = $state(null);
  let contacts = $state(null);
  let preview = $state(null); // { p, body, kind }
  let posted = $state(null);

  const WORD = { kunde: "kunder", leverandor: "leverandører" };

  function reset() {
    error = null;
    contacts = null;
    preview = null;
    posted = null;
  }

  async function send(path, file) {
    busy = true;
    reset();
    try {
      const body = await file.text();
      const r = await api(base + path, { method: "POST", headers: { "content-type": "text/csv" }, body });
      return { r, body };
    } catch (e) {
      error = e.message;
      return null;
    } finally {
      busy = false;
    }
  }

  async function contactsPicked(event) {
    const file = event.target.files[0];
    event.target.value = "";
    if (!file) return;
    const out = await send("contacts?kind=" + kind, file);
    if (out) contacts = out.r;
  }

  async function itemsPicked(event) {
    const file = event.target.files[0];
    event.target.value = "";
    if (!file) return;
    const out = await send("open-items?preview=true&kind=" + kind, file);
    if (out) preview = { p: out.r, body: out.body, kind };
  }

  async function post() {
    if (busy || !preview) return;
    const ok = await bekreft(
      preview.p.antall + " åpne poster, til sammen " + kr(preview.p.sum_ore) + " kr, bokføres som ett bilag på konto " +
        preview.p.konto + ". Et feilbokført bilag rettes med et reverseringsbilag.",
      { tittel: "Bokfør de åpne postene?", ok: "Bokfør" },
    );
    if (!ok) return;
    busy = true;
    try {
      const r = await api(base + "open-items?kind=" + preview.kind, {
        method: "POST",
        headers: { "content-type": "text/csv" },
        body: preview.body,
      });
      posted = { ...r, kind: preview.kind };
      preview = null;
      toast(r.antall + " åpne poster bokført som bilag " + r.voucher, true);
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }
</script>

<section class="card border border-base-300 bg-base-100 max-w-3xl" aria-labelledby="import-tittel">
  <div class="card-body gap-4">
    <h2 id="import-tittel" class="card-title">Importer fra et annet system</h2>
    <p class="text-sm opacity-70">
      SAF-T flytter hovedboken, men ikke kontaktopplysninger og åpne poster. Eksporter kunde- eller leverandørlisten og de
      åpne postene som CSV fra det gamle systemet. Kolonnene leses av overskriftene, og en fil vi ikke forstår sier hvilke
      kolonner den fant.
    </p>

    <fieldset class="fieldset">
      <legend class="fieldset-legend">Hva gjelder filen?</legend>
      <div class="flex flex-wrap gap-2">
        {#each [["kunde", "Kunder"], ["leverandor", "Leverandører"]] as [key, label] (key)}
          <label class="label cursor-pointer gap-2 border border-base-300 rounded-box px-3 py-2 {kind === key ? 'border-primary' : ''}">
            <input type="radio" class="radio radio-sm" name="import-kind" value={key} bind:group={kind} onchange={reset} />
            <span class="text-sm text-base-content">{label}</span>
          </label>
        {/each}
      </div>
    </fieldset>

    <div class="grid gap-4 sm:grid-cols-2">
      <div class="fieldset">
        <label class="fieldset-legend" for="import-kontakter">1. Kontaktliste for {WORD[kind]} (CSV)</label>
        <input id="import-kontakter" type="file" class="file-input w-full" accept=".csv,.txt" disabled={busy} onchange={contactsPicked} />
        <span class="label whitespace-normal">Navn, nummer, orgnr, e-post, adresse, kontonummer. Kan kjøres igjen: kjente parter oppdateres.</span>
      </div>
      <div class="fieldset">
        <label class="fieldset-legend" for="import-poster">2. Åpne poster for {WORD[kind]} (CSV)</label>
        <input id="import-poster" type="file" class="file-input w-full" accept=".csv,.txt" disabled={busy} onchange={itemsPicked} />
        <span class="label whitespace-normal">
          Restbeløp per faktura. Du ser postene før noe bokføres. De erstatter samlelinjen på {kind === "kunde" ? "1500" : "2400"},
          så utelat den kontoen fra åpningsbalansen.
        </span>
      </div>
    </div>

    <div aria-live="polite">
      {#if busy}
        <div class="flex items-center gap-2"><span class="loading loading-spinner loading-sm"></span> Leser filen …</div>
      {:else if error}
        <div class="alert alert-error" role="alert"><span>{error}</span></div>
      {:else if contacts}
        <div class="alert alert-success alert-soft" role="status">
          <div>
            <strong>{contacts.lest} rader lest:</strong> {contacts.opprettet} opprettet, {contacts.oppdatert} oppdatert.
            {#each contacts.warnings as w (w)}<span class="block text-sm">{w}</span>{/each}
            <a class="link block mt-1" href={"#/c/" + companyId + "/" + (kind === "kunde" ? "kunder" : "leverandorer")}>Se {WORD[kind]}</a>
          </div>
        </div>
      {:else if posted}
        <div class="alert alert-success alert-soft" role="status">
          <div>
            <strong>{posted.antall} åpne poster bokført som bilag {posted.voucher}</strong>, til sammen {kr(posted.sum_ore)} kr.
            {#each posted.warnings as w (w)}<span class="block text-sm">{w}</span>{/each}
            <a class="link block mt-1" href={"#/c/" + companyId + "/reskontro"}>Se om reskontroen stemmer</a>
          </div>
        </div>
      {:else if preview}
        {#if !preview.p.kan_importeres}
          <div class="alert alert-warning" role="alert">
            <span>
              Åpne poster kan bare importeres til en konto som står på null, og {preview.p.konto} har saldo
              {kr(preview.p.konto_saldo_ore)} kr. Er det en samlelinje fra åpningsbalansen, utelat eller reverser den og prøv igjen.
              Er postene alt importert, er det ingenting mer å gjøre.
            </span>
          </div>
        {:else}
          <h3 class="font-semibold">{preview.p.antall} åpne poster, til sammen {kr(preview.p.sum_ore)} kr på konto {preview.p.konto}</h3>
          {#if preview.p.nye_parter.length}
            <p class="text-sm">Opprettes som nye {WORD[preview.kind]}: {preview.p.nye_parter.join(", ")}</p>
          {/if}
          {#each preview.p.warnings as w (w)}<p class="text-sm text-warning">{w}</p>{/each}
          <div class="overflow-x-auto max-h-96 my-2">
            <table class="table table-sm table-pin-rows">
              <caption class="sr-only">Postene slik filen ble lest.</caption>
              <thead>
                <tr>
                  <th scope="col">Part</th>
                  <th scope="col" class="hidden sm:table-cell">Dokument</th>
                  <th scope="col" class="hidden md:table-cell">Forfall</th>
                  <th scope="col" class="text-right">Beløp</th>
                </tr>
              </thead>
              <tbody>
                {#each preview.p.rader as r, i (i)}
                  <tr>
                    <td>
                      {r.navn || r.part}
                      <span class="block text-xs opacity-70 sm:hidden">{r.dokument || ""}</span>
                    </td>
                    <td class="hidden sm:table-cell">{r.dokument || "–"}{r.kid ? ", KID " + r.kid : ""}</td>
                    <td class="hidden md:table-cell whitespace-nowrap">{r.forfall ? formatDate(r.forfall) : "–"}</td>
                    <td class="text-right tabular-nums whitespace-nowrap">{kr(r.belop_ore)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          {#if preview.p.antall > preview.p.rader.length}
            <p class="text-sm opacity-70">Viser de første {preview.p.rader.length}.</p>
          {/if}
          <div class="flex flex-wrap gap-2">
            <button class="btn btn-primary" disabled={busy} onclick={post}>Bokfør de åpne postene</button>
            <button class="btn btn-ghost" onclick={reset}>Avbryt</button>
          </div>
        {/if}
      {/if}
    </div>
  </div>
</section>
