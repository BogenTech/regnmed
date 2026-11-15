<script>
  // Skattemessig saldo per gruppe — beregnet fra registeret hver gang,
  // aldri lagret. Negativ saldo avskrives ikke; den er
  // inntektsføringskandidat og håndteres manuelt (sktl. §14-45/§14-47).
  //
  // The næringsspesifikasjon is filed in spring for the year before, so
  // until June the previous year comes first. A year the satsregister
  // does not cover fails loudly on the server, and that is shown.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import YearPicker from "../../components/YearPicker.svelte";

  let { companyId, versjon } = $props();

  const now = new Date();
  let year = $state(now.getMonth() < 5 ? now.getFullYear() - 1 : now.getFullYear());
  let saldo = $state(null); // null = loading, { error } = failed

  async function last() {
    const y = year;
    saldo = null;
    try {
      const svar = await api("/companies/" + companyId + "/assets/saldo?year=" + y);
      if (y === year) saldo = svar;
    } catch (error) {
      if (y === year) saldo = { error: error.message };
    }
  }

  $effect(() => {
    companyId;
    year;
    versjon;
    last();
  });
</script>

<Card title="Saldoavskrivning (skattemessig)">
  <div class="flex flex-wrap items-center gap-x-4 gap-y-2 mb-2">
    <YearPicker bind:year />
    <p class="text-sm opacity-70 flex-1 min-w-60">
      Grunnlaget for næringsspesifikasjonen: saldo per gruppe etter skatteloven §14-43. Negativ
      saldo avskrives ikke — den er inntektsføringskandidat og håndteres manuelt.
    </p>
  </div>

  {#if !saldo}
    <span class="loading loading-spinner loading-sm" aria-label="Laster saldoavskrivningen"></span>
  {:else if saldo.error}
    <LoadError error={saldo.error} retry={last} />
  {:else if !saldo.grupper.length}
    <p class="text-sm opacity-70">Ingen driftsmidler i saldogruppene for {saldo.year}.</p>
  {:else}
    <ul class="divide-y divide-base-200">
      {#each saldo.grupper as g (g.gruppe)}
        <li class="py-2 text-sm">
          <div class="font-medium">
            Gruppe {g.gruppe} <span class="font-normal opacity-70">{g.beskrivelse}, sats {g.sats_bp / 100} %</span>
          </div>
          <dl class="grid grid-cols-1 sm:grid-cols-6 gap-x-4 mt-1">
            {#each [["Inngående", g.inngaende_ore], ["Tilgang", g.tilgang_ore], ["Vederlag", g.vederlag_ore], ["Grunnlag", g.grunnlag_ore], ["Avskrivning", g.avskrivning_ore], ["Utgående", g.utgaende_ore]] as [navn, ore] (navn)}
              <div class="flex justify-between gap-2 sm:block sm:text-right">
                <dt class="opacity-60 text-xs">{navn}</dt>
                <dd class="tabular-nums whitespace-nowrap" class:font-medium={navn === "Utgående"}>{kr(ore)}</dd>
              </div>
            {/each}
          </dl>
        </li>
      {/each}
    </ul>
    <dl class="grid grid-cols-1 sm:grid-cols-3 gap-2 text-sm mt-3">
      <div>
        <dt class="opacity-60">Bokført verdi 31.12.{saldo.year}</dt>
        <dd class="font-medium">{kr(saldo.bokfort_ore)} kr</dd>
      </div>
      <div>
        <dt class="opacity-60">Skattemessig saldo</dt>
        <dd class="font-medium">{kr(saldo.skattemessig_ore)} kr</dd>
      </div>
      <div>
        <dt class="opacity-60">Midlertidig forskjell</dt>
        <dd class="font-medium">{kr(saldo.forskjell_ore)} kr</dd>
      </div>
    </dl>
  {/if}
</Card>
