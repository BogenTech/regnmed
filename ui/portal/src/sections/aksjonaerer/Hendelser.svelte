<script>
  import { post } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, hendelser, aksjonarer, typer, onDone } = $props();

  let holder = $state("");
  let type = $state("");
  let dato = $state(today());
  let antall = $state("");
  let belop = $state("");
  let motpart = $state("");

  // Velgerne peker alltid på noe som finnes: den første aksjonæren kan
  // bli registrert etter at skjemaet er tegnet opp, og da ville en tom
  // id blitt sendt selv om nedtrekket viste et navn.
  $effect(() => {
    if (!aksjonarer.some((a) => a.id === holder)) holder = aksjonarer[0]?.id || "";
    if (!typer.some((t) => t.slug === type)) type = typer[0]?.slug || "";
  });

  async function opprett() {
    const typeInfo = typer.find((t) => t.slug === type);
    const body = {
      shareholder_id: holder,
      type,
      dato,
      antall: parseInt(antall.trim(), 10),
      belop_ore: belop.trim() ? parseKr(belop.trim()) : null,
    };
    if (motpart) {
      body.motpart_id = motpart;
      // Motparten går motsatt vei av hovedhendelsen.
      body.motpart_type = typeInfo && typeInfo.tilgang ? "salg" : "kjop";
    }
    try {
      await post("/companies/" + companyId + "/share-events", body);
      toast("Hendelse registrert", true);
      dato = today();
      antall = "";
      belop = "";
      motpart = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Hendelser">
  <p class="text-sm opacity-70 mb-2">
    Hendelser er innsettings-bare, som bilag: en feilføring rettes med en motsatt hendelse, aldri
    ved å endre historien. En overdragelse registreres én gang — avgang hos selger og tilgang hos
    kjøper skrives i samme transaksjon. ⚠ merker typer vi ennå ikke kan levere til Skatteetaten (se
    Oppgaven under).
  </p>
  {#if hendelser.length}
    <div class="overflow-x-auto">
      <table class="table table-sm mb-4">
        <thead>
          <tr>
            <th>Dato</th>
            <th>Aksjonær</th>
            <th>Type</th>
            <th class="text-right">Antall</th>
            <th class="text-right">Beløp</th>
            <th>Motpart</th>
          </tr>
        </thead>
        <tbody>
          {#each hendelser as h}
            <tr>
              <td>{h.dato}</td>
              <td>{h.aksjonar}</td>
              <td>{h.type_navn}</td>
              <td class="text-right">{h.antall}</td>
              <td class="text-right">{h.belop_ore == null ? "" : kr(h.belop_ore)}</td>
              <td class="text-xs opacity-70">{h.motpart || ""}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
  {#if aksjonarer.length}
    <h3 class="font-semibold mb-1">Ny hendelse</h3>
    <div class="grid gap-2 max-w-md">
      <select class="select select-sm" bind:value={holder}>
        {#each aksjonarer as a (a.id)}
          <option value={a.id}>{a.navn} ({a.antall_aksjer})</option>
        {/each}
      </select>
      <select class="select select-sm" bind:value={type}>
        {#each typer as t (t.slug)}
          <!-- ⚠: transaksjonstypen kan registreres, men ikke leveres —
               en feil type flyter inn i aksjonærens RF-1088. -->
          <option value={t.slug}>{t.navn}{t.leverbar ? "" : " ⚠"}</option>
        {/each}
      </select>
      <div class="grid grid-cols-3 gap-2">
        <input type="date" class="input input-sm" bind:value={dato} />
        <input class="input input-sm" placeholder="Antall aksjer" bind:value={antall} />
        <input class="input input-sm" placeholder="Beløp (kr)" bind:value={belop} />
      </div>
      <select class="select select-sm" bind:value={motpart}>
        <option value="">Ingen motpart</option>
        {#each aksjonarer as a (a.id)}
          <option value={a.id}>{a.navn} ({a.antall_aksjer})</option>
        {/each}
      </select>
      <button class="btn btn-sm" onclick={opprett}>Registrer hendelse</button>
    </div>
  {/if}
</Card>
