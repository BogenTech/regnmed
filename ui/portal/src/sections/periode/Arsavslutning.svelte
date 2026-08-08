<script>
  // Årsavslutning (#84, docs/arsavslutning.md): resultatdisponering og
  // skattekostnad som ETT ordinært bilag. Hører hjemme ved siden av
  // periodelåsen fordi avslutningen låser året selv — den er halvparten
  // av den samme handlingen.
  import { api, post } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  // onavsluttet: avslutningen låser året, og låsen vises av kortet over —
  // den må få vite at den er utdatert.
  let { companyId, onavsluttet } = $props();

  let rader = $state(null);
  let ar = $state(new Date().getFullYear() - 1);
  let forslag = $state(null);
  let skatt = $state("");
  let lagrer = $state(false);

  const ore = (v) => Math.round(parseFloat(String(v).replace(",", ".") || "0") * 100);
  // Vist før innsending, så beløpet som havner i egenkapitalen ikke er
  // en overraskelse.
  let disponeres = $derived(
    forslag === null ? null : forslag.resultat_for_skatt_ore - ore(skatt),
  );

  async function last() {
    try {
      rader = (await api("/companies/" + companyId + "/arsavslutning")).arsavslutninger;
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function hentForslag() {
    forslag = null;
    try {
      forslag = await api("/companies/" + companyId + "/arsavslutning/" + ar + "/forslag");
    } catch (error) {
      toast(error.message, false);
    }
  }

  $effect(() => {
    void companyId;
    last();
  });

  async function avslutt() {
    lagrer = true;
    try {
      const svar = await post("/companies/" + companyId + "/arsavslutning", {
        ar: Number(ar),
        skattekostnad_ore: ore(skatt),
      });
      toast(
        ar + " avsluttet som bilag " + svar.bilag + " — " + kr(svar.disponert_ore) +
          " overført til egenkapital",
        true,
      );
      forslag = null;
      skatt = "";
      await last();
      onavsluttet?.();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }
</script>

<Card title="Årsavslutning">
  <p class="text-sm opacity-70 mb-2">
    Overfører årets resultat til opptjent egenkapital og avsetter skattekostnaden — ett ordinært
    bilag datert 31.12 (rskl. §6-1, §6-2). <strong>Avslutningen låser året selv</strong>, så et år
    som allerede er låst kan ikke avsluttes: bilaget ville ligget inne i låsen.
  </p>

  <div class="flex gap-2 mb-2 flex-wrap items-center">
    <input type="number" class="input input-sm w-28" bind:value={ar} />
    <button class="btn btn-sm" onclick={hentForslag}>Hent årets resultat</button>
  </div>

  {#if forslag}
    <div class="border border-base-200 rounded-box p-3 mb-2">
      <p class="mb-2">
        Resultat før skatt {ar}: <strong>{kr(forslag.resultat_for_skatt_ore)}</strong>
      </p>
      <label class="flex items-center gap-2 text-sm mb-1">
        Skattekostnad
        <input class="input input-sm w-36" placeholder="0,00" bind:value={skatt} />
      </label>
      <p class="text-xs opacity-70 mb-2">
        Oppgis av deg. Skattemessig resultat er ikke regnskapsmessig resultat — permanente og
        midlertidige forskjeller avgjør, og systemet dikter ikke opp en skattemelding. 0 er et
        gyldig svar.
      </p>
      <p class="mb-2">Overføres til egenkapital: <strong>{kr(disponeres)}</strong></p>
      <button class="btn btn-sm btn-primary" disabled={lagrer} onclick={avslutt}>
        Avslutt {ar}
      </button>
    </div>
  {/if}

  {#if rader && rader.length}
    <div class="overflow-x-auto">
      <table class="table table-sm">
        <thead>
          <tr>
            <th>År</th><th>Bilag</th>
            <th class="text-right">Før skatt</th>
            <th class="text-right">Skatt</th>
            <th class="text-right">Til egenkapital</th>
            <th>Avsluttet av</th>
          </tr>
        </thead>
        <tbody>
          {#each rader as a (a.ar)}
            <tr>
              <td>{a.ar}</td>
              <td>{a.bilag}</td>
              <td class="text-right">{kr(a.resultat_for_skatt_ore)}</td>
              <td class="text-right">{kr(a.skattekostnad_ore)}</td>
              <td class="text-right">{kr(a.disponert_ore)}</td>
              <td class="opacity-70">{a.created_by}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if rader}
    <p class="opacity-70 text-sm">Ingen år er avsluttet ennå.</p>
  {/if}
</Card>
