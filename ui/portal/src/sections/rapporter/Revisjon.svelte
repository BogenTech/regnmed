<script>
  // Revisorens verifikasjonsrapport (#24): seks kontroller kjøres og
  // AVVIK VISES SOM LINJER — en feilet kontroll skal aldri skjules
  // bak en feilmelding eller utelates fra rapporten.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";

  let { companyId } = $props();

  let data = $state(null);

  $effect(() => {
    data = null;
    api("/companies/" + companyId + "/reports/revisjon")
      .then((svar) => (data = svar))
      .catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-sm"></span>
{:else}
  <div class="alert {data.alle_ok ? 'alert-success' : 'alert-error'} mb-4">
    {data.alle_ok ? "Alle kontroller OK" : "Avvik funnet — se kontrollene under"}
    <span class="text-xs font-mono opacity-70">kjedehode: sekvens {data.kjede_sekvens}</span>
  </div>

  <table class="table table-sm mb-4">
    <tbody>
      {#each data.kontroller as k}
        <tr>
          <td>{k.ok ? "✓" : "✗"}</td>
          <td class="font-semibold">{k.navn}</td>
          <td>{k.detalj}</td>
        </tr>
      {/each}
    </tbody>
  </table>

  {#if data.ankere.length}
    <p class="text-sm font-semibold mb-1">Eksterne forankringer</p>
    <table class="table table-sm mb-4">
      <tbody>
        {#each data.ankere as a}
          <tr>
            <td>{a.tidspunkt.slice(0, 16).replace("T", " ")}</td>
            <td>sekvens {a.siste_sekvens}</td>
            <td class="font-mono text-xs break-all">
              {a.root}
              {#if a.vitner.length}
                <br />
                <span class="opacity-60">
                  {#each a.vitner as v, i}{#if i}<br />{/if}{v}{/each}
                </span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="text-sm opacity-70 mb-4">Ingen forankringer omfatter selskapet ennå.</p>
  {/if}

  <button
    class="btn btn-outline btn-sm"
    onclick={() =>
      download(
        "/companies/" + companyId + "/reports/revisjon?format=tekst",
        "verifikasjonsrapport.txt",
      )}
  >
    Last ned rapport (tekst)
  </button>
  <p class="text-xs opacity-60 mt-2">
    Rapporten kan etterprøves uavhengig av regnmed: kjeden re-beregnes fra genesis, røttene
    sammenlignes med den offentlige /anchors-strømmen, og RFC 3161-vitner verifiseres frakoblet.
  </p>
{/if}
