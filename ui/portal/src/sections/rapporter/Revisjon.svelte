<script>
  // Revisorens verifikasjonsrapport (#24): kontrollene kjøres og
  // AVVIK VISES SOM LINJER — en feilet kontroll skal aldri skjules
  // bak en feilmelding eller utelates fra rapporten.
  import { runJob } from "../../lib/jobs.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { saveText } from "../../lib/download.js";

  let { companyId } = $props();

  let data = $state(null);
  let status = $state("queued");

  // Every control re-walks the whole ledger, so the report is a
  // background job (docs/jobs.md) instead of a request held open.
  $effect(() => {
    data = null;
    const id = companyId;
    runJob(id, "audit-report", {}, (s) => (status = s))
      .then((result) => {
        if (id === companyId) data = result;
      })
      .catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <div class="flex items-center gap-2 text-sm" role="status">
    <span class="loading loading-spinner loading-sm"></span>
    {status === "running" ? "Kjører kontrollene over hele hovedboken …" : "Starter kontrollene …"}
  </div>
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
          <!-- En kontroll kan finne flere avvik; hvert står på sin egen
               linje i detaljteksten og skal vises slik. -->
          <td class="whitespace-pre-line">{k.detalj}</td>
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
    onclick={() => saveText(data.tekst, "verifikasjonsrapport_" + data.orgnr + ".txt")}
  >
    Last ned rapport (tekst)
  </button>
  <p class="text-xs opacity-60 mt-2">
    Rapporten kan etterprøves uavhengig av regnmed: kjeden re-beregnes fra genesis, røttene
    sammenlignes med den offentlige /anchors-strømmen, og RFC 3161-vitner verifiseres frakoblet.
  </p>
{/if}
