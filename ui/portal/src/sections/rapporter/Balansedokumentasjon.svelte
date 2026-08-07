<script>
  // Balansedokumentasjon (#88, docs/balansedokumentasjon.md):
  // bokføringsloven §11 — hva hver balansepost BESTÅR AV ved
  // periodeslutt. Én rad per balansekonto med saldo; udokumentert er et
  // AVVIK i revisjonsrapporten, så listen er arbeidslisten for å lukke
  // det.
  import { api, post } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, to } = $props();

  let periode = $state(to);
  let data = $state(null);
  let skjema = $state(null); // { konto, forklaring, fil }

  async function last() {
    data = null;
    try {
      data = await api(
        "/companies/" + companyId + "/balansedokumentasjon?periode=" + periode,
      );
    } catch (error) {
      toast(error.message, false);
    }
  }

  $effect(() => {
    void companyId;
    void periode;
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
      toast("Avstemmingen er registrert");
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
  <input type="date" class="input input-sm" bind:value={periode} />
</label>

{#if !data}
  <span class="loading loading-spinner loading-sm"></span>
{:else if !data.kontoer.length}
  <p class="opacity-70 text-sm">Ingen balansekontoer med saldo per {periode}.</p>
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
            <td class="text-right">{kr(k.saldo_ore)}</td>
            <td>
              {#if !k.avstemt}
                <span class="badge badge-error badge-sm">mangler</span>
              {:else}
                <div class="text-sm">
                  {k.avstemt.forklaring}
                  <span class="opacity-60">
                    — {k.avstemt.avstemt_av}, {k.avstemt.avstemt_dato}
                  </span>
                  {#if k.avstemt.har_vedlegg}
                    <a
                      class="link ml-1"
                      href={"/companies/" + companyId + "/balansedokumentasjon/" + k.avstemt.id + "/vedlegg"}
                    >
                      {k.avstemt.vedlegg_navn}
                    </a>
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
  <div class="mt-3 border border-base-200 rounded-box p-3">
    <p class="font-semibold mb-2">Avstem konto {skjema.konto} per {periode}</p>
    <input
      class="input input-sm w-full mb-2"
      placeholder="Hva består saldoen av? (kontoutskrift, tellelister, lånesaldo …)"
      bind:value={skjema.forklaring}
    />
    <input
      type="file"
      class="file-input file-input-sm w-full mb-2"
      onchange={(e) => (skjema.fil = e.currentTarget.files[0] || null)}
    />
    <div class="flex gap-2">
      <button class="btn btn-sm btn-primary" onclick={avstem}>Registrer</button>
      <button class="btn btn-sm btn-ghost" onclick={() => (skjema = null)}>Avbryt</button>
    </div>
  </div>
{/if}
