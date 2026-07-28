<script>
  import { toast } from "../../lib/toast.svelte.js";
  import { lastOppKvittering } from "../../lib/ko.js";
  import Card from "../../components/Card.svelte";
  import KoStatus from "./KoStatus.svelte";

  let { companyId } = $props();

  async function valgt(event) {
    const fil = event.target.files[0];
    if (!fil) return;
    event.target.value = "";
    try {
      const utfall = await lastOppKvittering(companyId, fil);
      toast(
        utfall === "sendt"
          ? "Kvitteringen ligger i innboksen"
          : "Uten dekning — kvitteringen er lagret og sendes automatisk",
        utfall === "sendt",
      );
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Send inn kvittering">
  <p class="text-sm opacity-70 mb-3">
    Ta bilde av en kvittering, så havner den i selskapets innboks. Den som fører regnskapet tar
    den derfra — du bokfører ingenting selv. Gjelder det et utlegg du skal ha igjen penger for,
    bruk <b>Utlegg</b> i stedet.
  </p>
  <label class="btn btn-sm btn-primary w-fit">
    Ta bilde av kvittering
    <input type="file" class="hidden" accept="image/*" capture="environment" onchange={valgt} />
  </label>
  <KoStatus />
</Card>
