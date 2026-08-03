<script>
  // Registrering. Aktiveringsgrensen gir en ADVARSEL fra serveren, aldri
  // en nekting — grensen er en vurdering, ikke en sperre, så svaret
  // vises som en del av kvitteringen på at driftsmidlet ble registrert.
  import { post } from "../../lib/api.js";
  import { parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, onDone } = $props();

  // Saldogruppene med satsene i sktl. §14-43 — teksten er valglisten,
  // selve satsen slås opp i satsregisteret på serveren.
  const SALDOGRUPPER = [
    ["a", "Kontormaskiner o.l. (30 %)"],
    ["b", "Forretningsverdi (20 %)"],
    ["c", "Vogntog, lastebiler, busser (24 %)"],
    ["d", "Personbiler, maskiner, inventar (20 %)"],
    ["e", "Skip, fartøyer (14 %)"],
    ["f", "Fly, helikopter (12 %)"],
    ["g", "Kraftanlegg (5 %)"],
    ["h", "Bygg og anlegg (4 %)"],
    ["i", "Forretningsbygg (2 %)"],
    ["j", "Teknisk installasjon i bygg (10 %)"],
  ];

  let navn = $state("");
  let dato = $state(today());
  let kostpris = $state("");
  let restverdi = $state("");
  let levetid = $state("");
  let balanse = $state("1250");
  let avskr = $state("6000");
  let gruppe = $state(SALDOGRUPPER[0][0]);

  function nullstill() {
    navn = "";
    dato = today();
    kostpris = "";
    restverdi = "";
    levetid = "";
    balanse = "1250";
    avskr = "6000";
    gruppe = SALDOGRUPPER[0][0];
  }

  async function opprett() {
    try {
      const made = await post("/companies/" + companyId + "/assets", {
        navn: navn.trim(),
        anskaffelsesdato: dato,
        kostpris_ore: parseKr(kostpris),
        restverdi_ore: restverdi.trim() ? parseKr(restverdi) : 0,
        levetid_maneder: parseInt(levetid, 10),
        balansekonto: balanse.trim(),
        avskrivningskonto: avskr.trim(),
        saldogruppe: gruppe,
      });
      toast(made.warning ? "Registrert — " + made.warning : "Driftsmiddel registrert", true);
      nullstill();
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<div class="grid gap-2 max-w-md">
  <input
    class="input input-sm"
    placeholder="Navn (f.eks. Varebil)"
    bind:value={navn}
  />
  <div class="grid grid-cols-3 gap-2">
    <input type="date" class="input input-sm" bind:value={dato} />
    <input class="input input-sm" placeholder="Kostpris (kr)" bind:value={kostpris} />
    <input
      class="input input-sm"
      placeholder="Restverdi (kr)"
      bind:value={restverdi}
    />
  </div>
  <div class="grid grid-cols-3 gap-2">
    <input class="input input-sm" placeholder="Levetid (mnd)" bind:value={levetid} />
    <input class="input input-sm" title="Balansekonto" bind:value={balanse} />
    <input class="input input-sm" title="Avskrivningskonto" bind:value={avskr} />
  </div>
  <select class="select select-sm" bind:value={gruppe}>
    {#each SALDOGRUPPER as g (g[0])}
      <option value={g[0]}>{g[0]} — {g[1]}</option>
    {/each}
  </select>
  <button class="btn btn-sm" onclick={opprett}>Registrer driftsmiddel</button>
</div>
