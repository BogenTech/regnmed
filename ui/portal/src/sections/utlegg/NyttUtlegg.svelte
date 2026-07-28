<script>
  // Kvitteringen sendes som rå kropp til endepunktet; beløp, dato og
  // formål følger med som query. Innsendingen ER opplastingen — derfor
  // krever vi at feltene er fylt ut FØR filvelgeren åpnes for alvor.
  import { api } from "../../lib/api.js";
  import { parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, onDone } = $props();

  let dato = $state(today());
  let belop = $state("");
  let formal = $state("");

  async function filValgt(event) {
    const file = event.target.files[0];
    if (!file) return;
    event.target.value = "";
    try {
      if (!belop.trim() || !formal.trim()) {
        throw new Error("fyll inn beløp og formål før du velger kvittering");
      }
      await api(
        "/companies/" +
          companyId +
          "/expenses/utlegg?filename=" +
          encodeURIComponent(file.name) +
          "&dato=" +
          dato +
          "&belop_ore=" +
          parseKr(belop) +
          "&beskrivelse=" +
          encodeURIComponent(formal),
        {
          method: "POST",
          headers: { "content-type": file.type || "application/octet-stream" },
          body: file,
        },
      );
      toast("Utlegg sendt inn", true);
      dato = today();
      belop = "";
      formal = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Nytt utlegg">
  <p class="text-sm opacity-70 mb-2">
    Kvitteringen er uforanderlig fra innsending og følger kravet inn på bilaget ved godkjenning
    (oppbevaringsplikt). Avvisning krever begrunnelse.
  </p>
  <div class="flex flex-wrap gap-2 items-end">
    <input type="date" class="input input-sm input-bordered" bind:value={dato} />
    <input class="input input-sm input-bordered w-28" placeholder="Beløp (kr)" bind:value={belop} />
    <input class="input input-sm input-bordered" placeholder="Formål" bind:value={formal} />
    <label class="btn btn-sm">
      Velg kvittering og send inn
      <input type="file" class="hidden" onchange={filValgt} />
    </label>
  </div>
</Card>
