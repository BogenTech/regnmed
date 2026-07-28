<script>
  // Dimensjonsregisteret (#37): koden er PERMANENT fordi den inngår i
  // bilagshashen — derfor kan en dimensjon bare avsluttes, aldri slettes.
  import { post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, dims, onDone } = $props();

  let kind = $state("avdeling");
  let code = $state("");
  let name = $state("");

  async function opprett() {
    try {
      await post("/companies/" + companyId + "/dimensions", {
        kind,
        code: code.trim(),
        name: name.trim(),
      });
      toast("Dimensjon opprettet", true);
      code = "";
      name = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function toggle(d) {
    try {
      await send(
        "PUT",
        "/companies/" + companyId + "/dimensions/" + d.kind + "/" + encodeURIComponent(d.code),
        { active: !d.active },
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Dimensjoner — avdeling og prosjekt">
  <p class="text-sm opacity-70 mb-2">
    Koden er permanent (den inngår i bilagshashen); navnet kan endres, og avsluttede dimensjoner
    avviser nye posteringer.
  </p>
  {#if dims.length}
    <table class="table table-xs mb-2">
      <thead>
        <tr><th>Type</th><th>Kode</th><th>Navn</th><th></th></tr>
      </thead>
      <tbody>
        {#each dims as d (d.kind + ":" + d.code)}
          <tr class={d.active ? "" : "opacity-50"}>
            <td>{d.kind}</td>
            <td>{d.code}</td>
            <td>{d.name}</td>
            <td>
              <button class="btn btn-xs btn-ghost" onclick={() => toggle(d)}>
                {d.active ? "Avslutt" : "Gjenåpne"}
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <div class="flex gap-2">
    <select class="select select-sm select-bordered" bind:value={kind}>
      <option value="avdeling">avdeling</option>
      <option value="prosjekt">prosjekt</option>
    </select>
    <input class="input input-sm input-bordered w-24" placeholder="Kode" bind:value={code} />
    <input class="input input-sm input-bordered" placeholder="Navn" bind:value={name} />
    <button class="btn btn-sm" onclick={opprett}>Opprett</button>
  </div>
</Card>
