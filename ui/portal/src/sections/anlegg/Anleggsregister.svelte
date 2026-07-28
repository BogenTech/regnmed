<script>
  // Registeret er bevis: driftsmidler registreres og avhendes, aldri
  // redigeres eller slettes. Bokført verdi lagres ALDRI — den er
  // kostpris − bokførte avskrivninger, utledet hver gang.
  import { api, post } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import NyttDriftsmiddel from "./NyttDriftsmiddel.svelte";

  let { companyId, assets, onDone } = $props();

  // Avskrivningshistorikken for ett driftsmiddel, hentet på forespørsel:
  // {navn, runs}.
  let historikk = $state(null);

  async function generer() {
    try {
      const result = await post("/companies/" + companyId + "/assets/depreciate", {});
      toast(
        result.generated +
          " avskrivning(er) bokført" +
          (result.failed ? ", " + result.failed + " feilet" : ""),
        result.failed === 0,
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Avhending er ENVEIS: kravet lukkes, gevinst eller tap bokføres, og
  // driftsmidlet kan aldri gjenåpnes.
  async function avhend(a) {
    const dato = prompt("Avhendingsdato (ÅÅÅÅ-MM-DD):", today());
    if (!dato) return;
    const vederlag = prompt("Vederlag (kr, 0 ved utrangering):", "0");
    if (vederlag === null) return;
    try {
      const result = await post(
        "/companies/" + companyId + "/assets/" + a.asset_id + "/dispose",
        { dato: dato.trim(), vederlag_ore: parseKr(vederlag) },
      );
      const g = result.gevinst_ore;
      toast(
        "Avhendet — " +
          (g > 0 ? "gevinst " + kr(g) : g < 0 ? "tap " + kr(-g) : "ingen gevinst/tap"),
        true,
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function visHistorikk(a) {
    try {
      const data = await api("/companies/" + companyId + "/assets/" + a.asset_id + "/runs");
      historikk = { navn: a.navn, runs: data.runs };
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Anleggsregister">
  <p class="text-sm opacity-70 mb-2">
    Registeret er bevis: driftsmidler registreres og avhendes, aldri redigeres eller slettes.
    Bokført verdi = kostpris − bokførte avskrivninger, alltid beregnet. Lineære avskrivninger
    bokføres månedlig som ordinære bilag (automatisk hver måned, eller med knappen).
  </p>
  <div class="mb-3">
    <button class="btn btn-sm btn-outline" onclick={generer}>
      Generer avskrivninger til i dag
    </button>
  </div>
  {#if assets.length}
    <table class="table table-sm mb-4">
      <thead>
        <tr>
          <th>Navn</th><th>Gr.</th><th>Anskaffet</th>
          <th class="text-right">Kostpris</th><th class="text-right">Avskrevet</th>
          <th class="text-right">Bokført</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each assets as a (a.asset_id)}
          <tr class={a.avhendet_dato ? "opacity-60" : ""}>
            <td>{a.navn}</td>
            <td>{a.saldogruppe}</td>
            <td>{a.anskaffelsesdato}</td>
            <td class="text-right">{kr(a.kostpris_ore)}</td>
            <td class="text-right">{kr(a.akkumulert_ore)}</td>
            <td class="text-right">
              {#if a.avhendet_dato}
                <span class="badge badge-ghost badge-sm">avhendet {a.avhendet_dato}</span>
              {:else}
                {kr(a.bokfort_ore)}
              {/if}
            </td>
            <td>
              <button class="btn btn-xs btn-ghost" onclick={() => visHistorikk(a)}>Historikk</button>
              {#if !a.avhendet_dato}
                <button class="btn btn-xs btn-outline" onclick={() => avhend(a)}>Avhend</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <div class="mb-3">
    {#if historikk}
      <h3 class="font-semibold mb-1">Avskrivninger — {historikk.navn}</h3>
      <table class="table table-xs max-w-md">
        <thead>
          <tr><th>Måned</th><th class="text-right">Beløp</th><th>Bilag</th></tr>
        </thead>
        <tbody>
          {#each historikk.runs as r (r.period)}
            <tr>
              <td>{r.period.slice(0, 7)}</td>
              <td class="text-right">{kr(r.amount_ore)}</td>
              <td>
                {#if r.voucher}
                  bilag {r.voucher}
                {:else}
                  <span class="text-error">{r.detail || "feilet"}</span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
  <h3 class="font-semibold mb-1">Nytt driftsmiddel</h3>
  <NyttDriftsmiddel {companyId} {onDone} />
</Card>
