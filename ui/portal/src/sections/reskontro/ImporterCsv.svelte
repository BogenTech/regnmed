<script>
  // Migrering: kontakter og åpne poster fra CSV (#19,
  // docs/migration.md). Kolonnene leses av OVERSKRIFTENE; åpne poster
  // forhåndsvises alltid før noe bokføres, og de ERSTATTER samlelinjen
  // på reskontrokontoen.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, onDone } = $props();

  let kind = $state("kunde");
  let busy = $state(false);
  let feil = $state(null);
  let kontaktResultat = $state(null);
  let preview = $state(null); // {p, body, kind}

  async function importCsv(path, file) {
    busy = true;
    feil = null;
    kontaktResultat = null;
    preview = null;
    try {
      const body = await file.text();
      const svar = await api("/companies/" + companyId + "/import/" + path + "&kind=" + kind, {
        method: "POST",
        headers: { "content-type": "text/csv" },
        body,
      });
      return { svar, body };
    } catch (error) {
      feil = error.message;
      return null;
    } finally {
      busy = false;
    }
  }

  async function kontakterValgt(event) {
    const file = event.target.files[0];
    if (!file) return;
    const result = await importCsv("contacts?", file);
    event.target.value = "";
    if (!result) return;
    kontaktResultat = result.svar;
    onDone();
  }

  async function posterValgt(event) {
    const file = event.target.files[0];
    if (!file) return;
    const result = await importCsv("open-items?preview=true&", file);
    event.target.value = "";
    if (!result) return;
    preview = { p: result.svar, body: result.body, kind };
  }

  async function bekreft() {
    try {
      const posted = await api("/companies/" + companyId + "/import/open-items?kind=" + preview.kind, {
        method: "POST",
        headers: { "content-type": "text/csv" },
        body: preview.body,
      });
      toast(posted.antall + " åpne poster bokført som bilag " + posted.voucher, true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Importer fra et annet system">
  <p class="text-sm opacity-70 mb-2">
    SAF-T flytter hovedboken, men ikke kontaktopplysninger og åpne poster. Eksporter
    kunde-/leverandørlisten og de åpne postene som CSV fra det gamle systemet — kolonnene leses
    av overskriftene, og en fil vi ikke forstår sier hvilke kolonner den fant.
  </p>
  <div class="flex gap-2 items-center flex-wrap mb-2">
    <select class="select select-sm select-bordered" bind:value={kind}>
      <option value="kunde">kunder</option>
      <option value="leverandor">leverandører</option>
    </select>
    <label class="btn btn-sm btn-outline">
      Kontaktliste (CSV)
      <input type="file" class="hidden" accept=".csv,.txt" onchange={kontakterValgt} />
    </label>
    <label class="btn btn-sm btn-outline">
      Åpne poster (CSV)
      <input type="file" class="hidden" accept=".csv,.txt" onchange={posterValgt} />
    </label>
  </div>
  <p class="text-xs opacity-60">
    Åpne poster ERSTATTER samlelinjen på reskontrokontoen: utelat 1500/2400 fra
    åpningsbalansen, så blir saldoen lik summen av postene. Du får en forhåndsvisning før noe
    bokføres.
  </p>
  <div class="mt-3">
    {#if busy}
      <span class="loading loading-spinner loading-sm"></span>
    {:else if feil}
      <div class="alert alert-error text-sm py-2">{feil}</div>
    {:else if kontaktResultat}
      <div class="alert alert-success text-sm py-2">
        {kontaktResultat.lest} rader lest: {kontaktResultat.opprettet} opprettet,
        {kontaktResultat.oppdatert} oppdatert.
        {#each kontaktResultat.warnings as w}<br />{w}{/each}
      </div>
    {:else if preview}
      {#if !preview.p.kan_importeres}
        <div class="alert alert-warning text-sm py-2">
          Konto {preview.p.konto} har allerede saldo {kr(preview.p.konto_saldo_ore)} — åpne
          poster erstatter samlelinjen. Utelat kontoen fra åpningsbalansen først.
        </div>
      {:else}
        <div class="border border-base-300 rounded-lg p-3">
          <p class="text-sm font-semibold mb-1">
            {preview.p.antall} åpne poster, sum {kr(preview.p.sum_ore)} på konto {preview.p.konto}
          </p>
          {#if preview.p.nye_parter.length}
            <p class="text-xs opacity-70 mb-2">
              Opprettes ved import: {preview.p.nye_parter.join(", ")}
            </p>
          {/if}
          {#if preview.p.warnings.length}
            <p class="text-xs opacity-70 mb-2">
              {#each preview.p.warnings as w, i}{#if i}<br />{/if}{w}{/each}
            </p>
          {/if}
          <button class="btn btn-sm btn-primary" onclick={bekreft}>Bokfør de åpne postene</button>
        </div>
      {/if}
    {/if}
  </div>
</Card>
