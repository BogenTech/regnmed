<script>
  // Maskin-tilgang: en integrasjon er en person av typen «integrasjon»,
  // så tilgangsoppslag og revisjonsspor er de samme som for mennesker.
  // Tilbakekalling virker straks (valid_to er eksklusiv).
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, integrasjoner, kall, onDone } = $props();

  let clientId = $state("");
  let navn = $state("");
  let access = $state("les");

  async function giTilgang() {
    try {
      await post("/companies/" + companyId + "/integrations", {
        client_id: clientId.trim(),
        navn: navn.trim(),
        access,
      });
      toast("Integrasjonen har fått tilgang", true);
      clientId = "";
      navn = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function trekkTilbake(i) {
    if (!confirm("Trekke tilbake tilgangen? Den slutter å virke med én gang.")) return;
    try {
      await post("/companies/" + companyId + "/integrations/" + i.integration_id + "/revoke", {});
      toast("Tilgangen er trukket tilbake", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Integrasjoner (maskin-tilgang)">
  <p class="text-sm opacity-70 mb-2">
    Et kassasystem eller en nettbutikk kan kalle API-et med sin egen identitet fra
    påloggingstjenesten. Du gir tilgangen, på det nivået du vil, og kan trekke den tilbake når som
    helst — den slutter å virke i samme øyeblikk. Alt roboten bokfører bærer navnet dens.
  </p>
  {#if integrasjoner.length}
    <table class="table table-sm mb-3">
      <thead>
        <tr>
          <th>Integrasjon</th><th>Nivå</th><th>Status</th>
          <th class="text-right">Kall i dag</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each integrasjoner as i (i.integration_id)}
          <tr class={i.aktiv ? "" : "opacity-50"}>
            <td>
              {i.navn}<br /><span class="text-xs opacity-60 font-mono">{i.client_id}</span>
            </td>
            <td>{i.access}</td>
            <td>
              {#if i.aktiv}
                <span class="badge badge-success badge-sm">aktiv</span>
              {:else}
                <span class="badge badge-ghost badge-sm">trukket {i.valid_to || ""}</span>
              {/if}
            </td>
            <td class="text-right">{i.kall_i_dag} / {i.rate_limit_min}/min</td>
            <td>
              {#if i.aktiv}
                <button class="btn btn-xs btn-ghost" onclick={() => trekkTilbake(i)}>
                  Trekk tilbake
                </button>
              {:else}
                {i.revoked_by || ""}
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen integrasjoner ennå.</p>
  {/if}
  <div class="flex gap-2 items-center flex-wrap">
    <input
      class="input input-sm w-56"
      placeholder="client_id fra regnid"
      bind:value={clientId}
    />
    <input class="input input-sm w-40" placeholder="Navn" bind:value={navn} />
    <select class="select select-sm" bind:value={access}>
      <option value="les">les</option>
      <option value="bokforing">bokføring</option>
    </select>
    <button class="btn btn-sm" onclick={giTilgang}>Gi tilgang</button>
  </div>
  {#if kall.length}
    <h3 class="font-semibold text-sm mt-3 mb-1">Siste endringer</h3>
    {#each kall.slice(0, 10) as k, i (i)}
      <div class="text-xs opacity-70 py-0.5">
        {k.tidspunkt.slice(0, 16).replace("T", " ")} · {k.navn} · {k.method} {k.path}
      </div>
    {/each}
  {/if}
</Card>
