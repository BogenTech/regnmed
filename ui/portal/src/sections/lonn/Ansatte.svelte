<script>
  import { post } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, ansatte, onDone } = $props();

  let navn = $state("");
  let fnr = $state("");
  let stilling = $state("");
  let fra = $state(today());
  let lonn = $state("");
  let trekk = $state("");
  let fp = $state("10.2");

  function nullstill() {
    navn = "";
    fnr = "";
    stilling = "";
    fra = today();
    lonn = "";
    trekk = "";
    fp = "10.2";
  }

  async function opprett() {
    try {
      await post("/companies/" + companyId + "/employees", {
        navn: navn.trim(),
        fodselsnummer: fnr.trim(),
        stilling: stilling.trim() || null,
        ansatt_fra: fra.trim(),
        manedslonn_ore: lonn.trim() ? parseKr(lonn.trim()) : null,
        trekk_type: trekk.trim() ? "prosent" : "ingen",
        trekk_prosent_bp: trekk.trim()
          ? Math.round(parseFloat(trekk.trim().replace(",", ".")) * 100)
          : null,
        feriepenger_bp: Math.round(parseFloat(fp.trim().replace(",", ".")) * 100),
      });
      toast("Ansatt registrert", true);
      nullstill();
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Ansatte">
  <p class="text-sm opacity-70 mb-2">
    Ansattregisteret. Identiteten (fødselsnummeret) er permanent og vises ikke her — listen viser
    <b>fødselsdato</b>, som er nok til å kjenne igjen en ansatt. Feriepengesatsen står per ansatt
    fordi den er et faktum om arbeidsforholdet: 10,2 % etter ferieloven §10, 12,5 % fra året man
    fyller 60, høyere på tariff.
  </p>
  {#if ansatte.length}
    <div class="overflow-x-auto">
      <table class="table table-sm mb-4">
        <thead>
          <tr>
            <th>Navn</th>
            <th>Stilling</th>
            <th>Født</th>
            <th class="text-right">Månedslønn</th>
            <th class="text-right">Trekk</th>
            <th class="text-right">Feriepenger</th>
          </tr>
        </thead>
        <tbody>
          {#each ansatte as a (a.id)}
            <tr class={a.ansatt_til && a.ansatt_til < today() ? "opacity-50" : ""}>
              <td>{a.navn}</td>
              <td class="text-xs opacity-70">{a.stilling || ""}</td>
              <!-- Ferieloven krever ikke fnr her; datoen er nok for å kjenne igjen. -->
              <td class="text-xs opacity-70">{a.fodselsdato || ""}</td>
              <td class="text-right">{a.manedslonn_ore == null ? "" : kr(a.manedslonn_ore)}</td>
              <td class="text-right">
                {#if a.trekk_type === "prosent"}
                  {(a.trekk_prosent_bp / 100).toFixed(1)} %
                {:else if a.trekk_type === "tabell"}
                  <!-- Tabelltrekk regnes IKKE ut av oss: trekktabellene er
                       Skatteetatens datafiler, og en tilnærming blir den
                       ansattes restskatt. Derfor merket, ikke beregnet. -->
                  <span class="text-warning">tabell {a.trekk_tabell} ⚠</span>
                {:else}
                  frikort
                {/if}
              </td>
              <td class="text-right">{(a.feriepenger_bp / 100).toFixed(1)} %</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen ansatte registrert.</p>
  {/if}

  <h3 class="font-semibold mb-1">Ny ansatt</h3>
  <div class="grid gap-2 max-w-md">
    <input class="input input-sm input-bordered" placeholder="Navn" bind:value={navn} />
    <div class="grid grid-cols-2 gap-2">
      <input
        class="input input-sm input-bordered"
        placeholder="Fødselsnummer (11 siffer)"
        bind:value={fnr}
      />
      <input class="input input-sm input-bordered" placeholder="Stilling" bind:value={stilling} />
    </div>
    <div class="grid grid-cols-2 gap-2">
      <input type="date" class="input input-sm input-bordered" bind:value={fra} />
      <input class="input input-sm input-bordered" placeholder="Månedslønn (kr)" bind:value={lonn} />
    </div>
    <div class="grid grid-cols-2 gap-2">
      <input
        class="input input-sm input-bordered"
        placeholder="Trekkprosent (f.eks. 35)"
        bind:value={trekk}
      />
      <input
        class="input input-sm input-bordered"
        title="Feriepengesats i prosent"
        bind:value={fp}
      />
    </div>
    <button class="btn btn-sm" onclick={opprett}>Registrer ansatt</button>
  </div>
</Card>
