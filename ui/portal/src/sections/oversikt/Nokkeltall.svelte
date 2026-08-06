<script>
  // Nøkkeltall (#36): rene SUM-tall fra serveren, CSS-søyler uten
  // diagrambibliotek — frugality.
  import { kr } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";

  let { tall } = $props();

  const MND_NAVN = ["jan", "feb", "mar", "apr", "mai", "jun", "jul", "aug", "sep", "okt", "nov", "des"];

  let maxAbs = $derived(Math.max(...tall.maaneder.map(Math.abs), 1));
  let diff = $derived(tall.resultat_hittil_ore - tall.resultat_fjor_ore);
</script>

<Card title={"Nøkkeltall " + tall.year}>
  <div class="stats stats-vertical lg:stats-horizontal bg-base-200 w-full mb-3">
    <div class="stat">
      <div class="stat-title">Resultat hittil i år</div>
      <div class="stat-value text-xl">{kr(tall.resultat_hittil_ore)}</div>
      <div class="stat-desc">
        {diff >= 0 ? "+" : "−"}{kr(Math.abs(diff))} mot i fjor ({kr(tall.resultat_fjor_ore)})
      </div>
    </div>
    <div class="stat">
      <div class="stat-title">Disponibelt om alt gjøres opp</div>
      <div class="stat-value text-xl {tall.likviditet.disponibelt_ore < 0 ? 'text-error' : ''}">
        {kr(tall.likviditet.disponibelt_ore)}
      </div>
      <div class="stat-desc">
        bank {kr(tall.likviditet.bank_ore)} + kunder {kr(tall.likviditet.kundefordringer_ore)} −
        leverandører {kr(tall.likviditet.leverandorgjeld_ore)} − mva {kr(tall.likviditet.mva_netto_ore)}
      </div>
    </div>
    <div class="stat">
      <div class="stat-title">Kommende frister</div>
      <div class="stat-value text-base">
        {tall.frister.length
          ? tall.frister.map((f) => f.label + " — " + f.frist).join(" · ")
          : "ingen kommende frister"}
      </div>
      <div class="stat-desc">mva-melding etter selskapets terminordning</div>
    </div>
  </div>
  <div class="flex gap-2 items-end">
    {#each tall.maaneder as v, i}
      <div class="flex flex-col items-center justify-end gap-1" style="height:64px">
        <div
          style={"height:" + (Math.round((Math.abs(v) / maxAbs) * 48) || 1) + "px"}
          class="w-4 rounded-t {v >= 0 ? 'bg-primary' : 'bg-error'}"
          title={MND_NAVN[i] + ": " + kr(v)}
        ></div>
        <span class="text-[10px] opacity-60">{MND_NAVN[i]}</span>
      </div>
    {/each}
  </div>
</Card>
