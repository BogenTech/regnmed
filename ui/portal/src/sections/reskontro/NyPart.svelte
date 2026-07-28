<script>
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, onDone } = $props();

  let kind = $state("kunde");
  let name = $state("");
  let orgnr = $state("");

  async function opprett(event) {
    event.preventDefault();
    try {
      const created = await post("/companies/" + companyId + "/parties", {
        kind,
        name,
        orgnr: orgnr || null,
      });
      toast("Part " + created.party_no + " opprettet", true);
      name = "";
      orgnr = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Ny part">
  <form class="flex flex-wrap gap-2 items-end" onsubmit={opprett}>
    <select class="select select-bordered" bind:value={kind}>
      <option value="kunde">kunde</option>
      <option value="leverandor">leverandør</option>
    </select>
    <input class="input input-bordered" placeholder="Navn" required bind:value={name} />
    <input class="input input-bordered w-32" placeholder="Orgnr (valgfritt)" bind:value={orgnr} />
    <button class="btn btn-primary">Opprett</button>
  </form>
</Card>
