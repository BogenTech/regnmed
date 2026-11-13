<script>
  // Balansedokumentasjon (#88, docs/balansedokumentasjon.md):
  // bokføringsloven §11 — hva hver balansepost BESTÅR AV ved
  // periodeslutt. Én rad per balansekonto med saldo; udokumentert er et
  // AVVIK i revisjonsrapporten, så listen er arbeidslisten for å lukke
  // det.
  import { api, post } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import LoadError from "../../components/LoadError.svelte";

  let { companyId, to, params } = $props();

  // The period end rides in the address; the year's last day otherwise.
  let periode = $derived(params.periode || to);
  let data = $state(null);
  let error = $state(null);
  let skjema = $state(null); // { konto, forklaring, fil }
  let seq = 0;

  function last() {
    const mine = ++seq;
    data = null;
    error = null;
    return api("/companies/" + companyId + "/balansedokumentasjon?periode=" + periode)
      .then((r) => mine === seq && (data = r))
      .catch((e) => mine === seq && (error = e.message));
  }
  $effect(() => {
    last();
  });

  async function avstem() {
    try {
      if (skjema.fil) {
        // Vedlegget ER dokumentasjonen — det lastes opp i samme kall som
        // påstanden om saldoen, ikke som et etterpå-steg.
        const q = new URLSearchParams({
          konto: skjema.konto,
          periode,
          forklaring: skjema.forklaring,
          filename: skjema.fil.name,
        });
        await api("/companies/" + companyId + "/balansedokumentasjon/vedlegg?" + q, {
          method: "POST",
          headers: { "content-type": skjema.fil.type || "application/octet-stream" },
          body: skjema.fil,
        });
      } else {
        await post("/companies/" + companyId + "/balansedokumentasjon", {
          konto: skjema.konto,
          periode,
          forklaring: skjema.forklaring,
        });
      }
      skjema = null;
      toast("Avstemmingen er registrert", true);
      await last();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<p class="text-sm opacity-70 mb-2">
  For hver balansepost skal det foreligge dokumentasjon av saldoen ved periodeslutt
  (bokføringsloven §11). Manglende dokumentasjon er et <strong>avvik</strong> i
  verifikasjonsrapporten. Kontoer som ender på null er utelatt — det er ingenting å dokumentere.
</p>

<label class="flex items-center gap-2 text-sm mb-3">
  Periodeslutt
  <input
    type="date"
    class="input input-sm w-auto"
    value={periode}
    onchange={(e) => (params.periode = e.currentTarget.value && e.currentTarget.value !== to ? e.currentTarget.value : undefined)}
  />
</label>

{#if error}
  <LoadError {error} retry={last} />
{:else if !data}
  <span class="loading loading-spinner loading-sm" role="status" aria-label="Henter balansekontoene"></span>
{:else if !data.kontoer.length}
  <p class="opacity-70 text-sm">Ingen balansekontoer med saldo per {formatDate(periode)}.</p>
{:else}
  <div class="overflow-x-auto">
    <table class="table table-sm">
      <thead>
        <tr>
          <th>Konto</th>
          <th class="text-right">Saldo</th>
          <th>Dokumentasjon</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each data.kontoer as k (k.konto)}
          <tr>
            <td class="whitespace-nowrap">{k.konto} {k.kontonavn}</td>
            <td class="text-right tabular-nums whitespace-nowrap">{kr(k.saldo_ore)}</td>
            <td>
              {#if !k.avstemt}
                <span class="badge badge-error badge-sm">mangler</span>
              {:else}
                <div class="text-sm">
                  {k.avstemt.forklaring}
                  <span class="opacity-60">
                    — {k.avstemt.avstemt_av}, {formatDate(k.avstemt.avstemt_dato)}
                  </span>
                  {#if k.avstemt.har_vedlegg}
                    <!-- The API needs the bearer token, so a plain href would
                         answer 401: fetch it through the client instead. -->
                    <button
                      class="link ml-1"
                      onclick={() =>
                        download(
                          "/companies/" + companyId + "/balansedokumentasjon/" + k.avstemt.id + "/vedlegg",
                          k.avstemt.vedlegg_navn,
                        )}
                    >
                      {k.avstemt.vedlegg_navn}
                    </button>
                  {/if}
                </div>
                {#if k.avvik_ore}
                  <!-- Avstemt, og så bokført videre. Ikke det samme som
                       udokumentert, og sies derfor med egne ord. -->
                  <span class="badge badge-warning badge-sm">
                    bokført {kr(k.avvik_ore)} etter avstemming
                  </span>
                {/if}
              {/if}
            </td>
            <td class="text-right">
              <button
                class="btn btn-xs"
                aria-label={(k.avstemt ? "Avstem på nytt " : "Avstem ") + k.konto + " " + k.kontonavn}
                onclick={() => (skjema = { konto: k.konto, forklaring: "", fil: null })}
              >
                {k.avstemt ? "Avstem på nytt" : "Avstem"}
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

{#if skjema}
  <form
    class="mt-3 border border-base-200 rounded-box p-3"
    onsubmit={(e) => {
      e.preventDefault();
      avstem();
    }}
  >
    <p class="font-semibold mb-2">Avstem konto {skjema.konto} per {formatDate(periode)}</p>
    <label class="flex flex-col gap-1 text-sm mb-2">
      Hva består saldoen av?
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="input input-sm w-full"
        placeholder="Kontoutskrift, tellelister, lånesaldo …"
        required
        autofocus
        bind:value={skjema.forklaring}
      />
    </label>
    <label class="flex flex-col gap-1 text-sm mb-2">
      Dokumentasjon (valgfritt vedlegg)
      <input
        type="file"
        class="file-input file-input-sm w-full"
        onchange={(e) => (skjema.fil = e.currentTarget.files[0] || null)}
      />
    </label>
    <div class="flex gap-2">
      <button type="submit" class="btn btn-sm btn-primary">Registrer avstemmingen</button>
      <button type="button" class="btn btn-sm btn-ghost" onclick={() => (skjema = null)}>Avbryt</button>
    </div>
  </form>
{/if}
