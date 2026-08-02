<script>
  // Dimensjonsregisteret (#37): koden er PERMANENT fordi den inngår i
  // bilagshashen — derfor kan en dimensjon bare avsluttes, aldri slettes.
  // Et prosjekt kan knyttes til KUNDEN det er for (#80) — redigerbar
  // metadata på linje med navnet, aldri del av kjeden. Én kunde per
  // prosjekt; et prosjekt for flere kunder er to prosjekter.
  import { api, post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, dims, onDone } = $props();

  let kunder = $state([]);
  $effect(() => {
    api("/companies/" + companyId + "/parties?kind=kunde")
      .then((svar) => (kunder = svar.parties || []))
      .catch(() => {
        /* uten reskontroliste vises bare registeret */
      });
  });

  let kind = $state("avdeling");
  let code = $state("");
  let name = $state("");
  let nyKunde = $state("");

  async function opprett() {
    try {
      await post("/companies/" + companyId + "/dimensions", {
        kind,
        code: code.trim(),
        name: name.trim(),
        kunde: kind === "prosjekt" && nyKunde ? nyKunde : null,
      });
      toast("Dimensjon opprettet", true);
      code = "";
      name = "";
      nyKunde = "";
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

  async function settKunde(d, partyNo) {
    try {
      await send(
        "PUT",
        "/companies/" + companyId + "/dimensions/" + d.kind + "/" + encodeURIComponent(d.code),
        { kunde: partyNo },
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
      onDone(); // sett velgeren tilbake til det serveren faktisk mener
    }
  }
</script>

<Card title="Dimensjoner — avdeling og prosjekt">
  <p class="text-sm opacity-70 mb-2">
    Koden er permanent (den inngår i bilagshashen); navnet kan endres, og avsluttede dimensjoner
    avviser nye posteringer. Et prosjekt kan knyttes til kunden det er for — da kan timene følges
    til kunden, og fakturagrunnlaget foreslår mottaker. Lønnsomheten per prosjekt står under
    <a class="link" href={"#/c/" + companyId + "/rapporter/prosjekt"}>Rapporter → Prosjekt</a>.
  </p>
  {#if dims.length}
    <table class="table table-xs mb-2">
      <thead>
        <tr><th>Type</th><th>Kode</th><th>Navn</th><th>Kunde</th><th></th></tr>
      </thead>
      <tbody>
        {#each dims as d (d.kind + ":" + d.code)}
          <tr class={d.active ? "" : "opacity-50"}>
            <td>{d.kind}</td>
            <td>{d.code}</td>
            <td>{d.name}</td>
            <td>
              {#if d.kind === "prosjekt" && kunder.length}
                <select
                  class="select select-xs select-bordered"
                  value={d.kunde || ""}
                  onchange={(e) => settKunde(d, e.currentTarget.value)}
                >
                  <option value="">—</option>
                  {#each kunder as p (p.party_no)}
                    <option value={p.party_no}>{p.party_no} {p.name}</option>
                  {/each}
                </select>
              {:else if d.kunde}
                {d.kunde} {d.kunde_navn}
              {/if}
            </td>
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
  <div class="flex gap-2 flex-wrap">
    <select class="select select-sm select-bordered" bind:value={kind}>
      <option value="avdeling">avdeling</option>
      <option value="prosjekt">prosjekt</option>
    </select>
    <input class="input input-sm input-bordered w-24" placeholder="Kode" bind:value={code} />
    <input class="input input-sm input-bordered" placeholder="Navn" bind:value={name} />
    {#if kind === "prosjekt" && kunder.length}
      <select class="select select-sm select-bordered" bind:value={nyKunde}>
        <option value="">— ingen kunde —</option>
        {#each kunder as p (p.party_no)}
          <option value={p.party_no}>{p.party_no} {p.name}</option>
        {/each}
      </select>
    {/if}
    <button class="btn btn-sm" onclick={opprett}>Opprett</button>
  </div>
</Card>
