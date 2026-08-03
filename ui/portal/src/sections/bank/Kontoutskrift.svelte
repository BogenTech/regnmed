<script>
  // Kontoutskrift inn: camt.053-XML eller CSV — samme endepunkt, samme
  // matchemotor; serveren avgjør formatet av innholdet, ikke av navnet.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, account, onDone } = $props();

  async function filValgt(event) {
    const file = event.target.files[0];
    if (!file) return;
    event.target.value = "";
    try {
      const result = await api("/companies/" + companyId + "/bank/statements?account=" + account, {
        method: "POST",
        body: await file.text(),
      });
      toast(
        result.transactions + " transaksjoner, " + result.auto_matched + " koblet automatisk",
        true,
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Kontoutskrift (camt.053 eller CSV)">
  <input
    type="file"
    class="file-input"
    accept=".xml,.csv,.txt"
    onchange={filValgt}
  />
  <p class="text-sm opacity-70 mt-2">
    Last ned fra nettbanken (camt.053-XML eller CSV-eksport med kolonneoverskrifter) og last opp
    her — konto {account}.
  </p>
</Card>
