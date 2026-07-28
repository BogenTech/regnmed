<script>
  import { download } from "../../lib/download.js";
  import Card from "../../components/Card.svelte";

  let { companyId, year, termin } = $props();
</script>

<Card title="Eksport">
  <div class="flex gap-2 flex-wrap">
    <button
      class="btn btn-outline"
      onclick={() =>
        download(
          "/companies/" + companyId + "/reports/mva-melding?year=" + year + "&termin=" + termin,
          "mva-melding-" + year + "-termin" + termin + ".xml",
        )}
    >
      Mva-melding (XML)
    </button>
    <button
      class="btn btn-outline"
      onclick={() =>
        download(
          "/companies/" + companyId + "/reports/saft?year=" + year,
          "saf-t-" + year + ".xml",
        )}
    >
      SAF-T {year} (XML)
    </button>
  </div>
  <p class="text-sm opacity-70 mt-2">
    Filene genereres direkte fra hovedboken og validerer mot Skatteetatens skjema.
  </p>
</Card>
