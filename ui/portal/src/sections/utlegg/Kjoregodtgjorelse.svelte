<script>
  // Beløpet regnes av serveren med statens sats PÅ KJØREDATOEN, og
  // satsene lagres på kravet ved innsending. Den trekkpliktige delen
  // meldes tilbake med det samme — den skal aldri skjules.
  import { post } from "../../lib/api.js";
  import { kr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, onDone } = $props();

  let dato = $state(today());
  let km = $state("");
  let formal = $state("");

  async function registrer() {
    try {
      const made = await post("/companies/" + companyId + "/expenses/kjoring", {
        dato,
        km: parseInt(km, 10),
        beskrivelse: formal.trim(),
      });
      toast(
        "Kjøring registrert — " +
          kr(made.belop_ore) +
          " kr" +
          (made.trekkpliktig_ore > 0
            ? " (trekkpliktig del " + kr(made.trekkpliktig_ore) + ")"
            : ""),
        true,
      );
      dato = today();
      km = "";
      formal = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Kjøregodtgjørelse">
  <p class="text-sm opacity-70 mb-2">
    Beløpet beregnes med statens sats på kjøredatoen fra satsregisteret; den trekkfrie delen skilles
    fra den trekkpliktige, som varsles tydelig (lønnsinnberetning kommer med a-melding).
  </p>
  <div class="flex flex-wrap gap-2 items-end">
    <input type="date" class="input input-sm" bind:value={dato} />
    <input class="input input-sm w-24" placeholder="Km" bind:value={km} />
    <input
      class="input input-sm"
      placeholder="Strekning og formål"
      bind:value={formal}
    />
    <button class="btn btn-sm" onclick={registrer}>Registrer kjøring</button>
  </div>
</Card>
