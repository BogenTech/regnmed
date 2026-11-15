<script>
  // Registrering. Aktiveringsgrensen gir en ADVARSEL fra serveren, aldri
  // en nekting — grensen er en vurdering, ikke en sperre, så svaret
  // vises som en del av kvitteringen på at driftsmidlet ble registrert.
  import { tick } from "svelte";
  import { post } from "../../lib/api.js";
  import { today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import KontoVelger from "../../components/KontoVelger.svelte";
  import { SALDOGRUPPER, lesDriftsmiddel } from "./anlegg.js";

  let { companyId, kontoer, onCreated } = $props();

  const tomt = () => ({
    navn: "",
    dato: today(),
    kostpris: "",
    restverdi: "",
    levetid: "",
    balanse: "1250",
    avskr: "6000",
    gruppe: "d",
  });
  let felt = $state(tomt());
  let lagrer = $state(false);

  async function opprett(event) {
    event.preventDefault();
    if (lagrer) return;
    const lest = lesDriftsmiddel(felt);
    if (lest.feil) return toast(lest.feil, false);
    lagrer = true;
    try {
      const made = await post("/companies/" + companyId + "/assets", lest.body);
      toast(
        "«" + lest.body.navn + "» er registrert" + (made.warning ? ". Merk: " + made.warning : ""),
        true,
      );
      felt = tomt();
      await onCreated();
      await tick();
      document.getElementById("anlegg-navn")?.focus();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }
</script>

<form class="grid gap-x-3 gap-y-1 sm:grid-cols-3 text-sm max-w-2xl" onsubmit={opprett}>
  <label class="fieldset py-0 sm:col-span-2">
    <span class="fieldset-legend">Navn</span>
    <input
      id="anlegg-navn"
      class="input input-sm w-full"
      placeholder="For eksempel Varebil"
      required
      bind:value={felt.navn}
    />
  </label>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Anskaffet</span>
    <input type="date" class="input input-sm w-full" required bind:value={felt.dato} />
  </label>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Kostpris (kr)</span>
    <input class="input input-sm w-full" inputmode="decimal" required bind:value={felt.kostpris} />
  </label>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Restverdi (kr, valgfritt)</span>
    <input class="input input-sm w-full" inputmode="decimal" bind:value={felt.restverdi} />
  </label>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Levetid (måneder)</span>
    <input class="input input-sm w-full" inputmode="numeric" required bind:value={felt.levetid} />
  </label>
  <label class="fieldset py-0 sm:col-span-3">
    <span class="fieldset-legend">Saldogruppe (skattemessig)</span>
    <select class="select select-sm w-full" bind:value={felt.gruppe}>
      {#each SALDOGRUPPER as [kode, navn, sats] (kode)}
        <option value={kode}>{kode}: {navn} ({sats} %)</option>
      {/each}
    </select>
  </label>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Balansekonto</span>
    <KontoVelger
      id="anlegg-balanse"
      cls="input input-sm w-full"
      kontoer={kontoer.kontoer}
      standard={kontoer.standard}
      bind:value={felt.balanse}
    />
  </label>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Avskrivningskonto</span>
    <KontoVelger
      id="anlegg-avskr"
      cls="input input-sm w-full"
      kontoer={kontoer.kontoer}
      standard={kontoer.standard}
      bind:value={felt.avskr}
    />
  </label>
  <div class="flex items-end pb-1">
    <button class="btn btn-sm btn-primary" disabled={lagrer}>Registrer driftsmiddel</button>
  </div>
</form>
