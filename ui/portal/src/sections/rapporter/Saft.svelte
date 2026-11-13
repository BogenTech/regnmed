<script>
  // SAF-T Financial for the chosen year: the standard file the
  // bokføringspliktige hands over on request (bokføringsforskriften
  // § 7-8), generated from the ledger and validated against Skatteetaten's
  // schema. Moved here from Mva: it covers the whole ledger, not VAT.
  import { download } from "../../lib/download.js";

  let { companyId, year } = $props();
</script>

<p class="mb-3">
  Hele regnskapet for {year} i Skatteetatens standardformat. Filen lages fra hovedboken når du laster den ned, og
  kontrolleres mot Skatteetatens skjema.
</p>
<button
  class="btn btn-primary"
  onclick={() => download("/companies/" + companyId + "/reports/saft?year=" + year, "saf-t-" + year + ".xml")}
>
  Last ned SAF-T {year}
</button>
