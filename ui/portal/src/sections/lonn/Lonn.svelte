<script>
  // Lønn (#46): ansattregister + lønnskjøring. Kjøringen er
  // regnskapsføring, ikke rapportering — a-meldingen leveres ikke
  // herfra (docs/lonn.md).
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Ansatte from "./Ansatte.svelte";
  import Lonnskjoring from "./Lonnskjoring.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [employees, payroll] = await Promise.all([
      api("/companies/" + id + "/employees"),
      api("/companies/" + id + "/payroll"),
    ]);
    data = { ansatte: employees.ansatte, kjoringer: payroll.kjoringer };
  }

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    data = null;
    load(companyId).catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <Ansatte {companyId} ansatte={data.ansatte} onDone={reload} />
  <Lonnskjoring {companyId} ansatte={data.ansatte} kjoringer={data.kjoringer} onDone={reload} />
{/if}
