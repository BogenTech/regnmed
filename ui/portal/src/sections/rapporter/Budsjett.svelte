<script>
  // Budsjett og avvik (#41): budsjettet er den eneste MENINGEN i
  // systemet. Et utkast er fritt redigerbart; fastsettelse fryser
  // versjonen, og en revisjon blir en NY versjon — derfor kan
  // avviksrapporten alltid navngi planen den måler mot.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { kr } from "../../lib/format.js";
  import BudsjettRutenett from "./BudsjettRutenett.svelte";
  import Avvik from "./Avvik.svelte";

  let { companyId, year } = $props();

  let valgtBudsjett = $state("");
  let data = $state(null);

  let navn = $state("");
  let fraFjor = $state(false);
  let prosent = $state("0");

  async function load(id, ar, valgt) {
    const [budsjetter, av] = await Promise.all([
      api("/companies/" + id + "/budgets?year=" + ar),
      api(
        "/companies/" + id + "/reports/avvik?year=" + ar + (valgt ? "&budget_id=" + valgt : ""),
      ),
    ]);
    data = { budsjetter: budsjetter.budgets, av };
  }

  function reload() {
    load(companyId, year, valgtBudsjett).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    data = null;
    load(companyId, year, valgtBudsjett).catch((error) => toast(error.message, false));
  });

  // Uten et uttrykkelig valg måles det mot den versjonen avviksrapporten
  // selv oppgir — den nyeste fastsatte, ellers det nyeste utkastet.
  let aktivt = $derived(
    !data
      ? null
      : valgtBudsjett
        ? data.budsjetter.find((b) => b.budget_id === valgtBudsjett)
        : data.av.budsjett
          ? data.budsjetter.find((b) => b.budget_id === data.av.budsjett.budget_id)
          : null,
  );

  async function nyttBudsjett() {
    try {
      const created = await post("/companies/" + companyId + "/budgets", {
        year,
        navn: navn.trim() || null,
        fra_ar: fraFjor ? year - 1 : null,
        justering_bp: Math.round(Number(String(prosent).replace(",", ".") || 0) * 100),
      });
      toast("Budsjett opprettet", true);
      navn = "";
      valgtBudsjett = created.budget_id;
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function fastsett(b) {
    try {
      await post("/companies/" + companyId + "/budgets/" + b.budget_id + "/fastsett", {});
      toast("Budsjettet er fastsatt", true);
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function forkast(b) {
    try {
      await api("/companies/" + companyId + "/budgets/" + b.budget_id, { method: "DELETE" });
      toast("Utkastet er forkastet", true);
      valgtBudsjett = "";
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#if !data}
  <span class="loading loading-spinner loading-sm"></span>
{:else}
  <p class="text-sm opacity-70 mb-2">
    Et budsjett er et arbeidsdokument mens det er utkast. Fastsettelse fryser versjonen — en
    revisjon blir en ny versjon, slik at avviksrapporten alltid kan navngi hva den sammenligner
    mot.
  </p>

  {#if data.budsjetter.length}
    <table class="table table-sm mb-3">
      <thead>
        <tr>
          <th>Versjon</th><th>Navn</th><th>Status</th>
          <th class="text-right">Sum</th><th>Laget / fastsatt av</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each data.budsjetter as b (b.budget_id)}
          <tr class={aktivt && b.budget_id === aktivt.budget_id ? "bg-base-200" : ""}>
            <td>v{b.versjon}</td>
            <td>{b.navn}</td>
            <td>
              <span class="badge badge-sm {b.status === 'fastsatt' ? 'badge-success' : 'badge-warning'}">
                {b.status}
              </span>
            </td>
            <td class="text-right">{kr(b.sum_ore)}</td>
            <td class="text-xs opacity-70">
              {b.created_by}{b.fastsatt_by ? " → " + b.fastsatt_by : ""}
            </td>
            <td>
              <button class="btn btn-xs btn-ghost" onclick={() => (valgtBudsjett = b.budget_id)}>
                Velg
              </button>
              {#if b.status === "utkast"}
                <button class="btn btn-xs btn-outline" onclick={() => fastsett(b)}>Fastsett</button>
                <button class="btn btn-xs btn-ghost" onclick={() => forkast(b)}>Forkast</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen budsjetter for {year} ennå.</p>
  {/if}

  <div class="flex gap-2 items-center flex-wrap">
    <input class="input input-sm input-bordered w-48" placeholder="Navn (valgfritt)" bind:value={navn} />
    <label class="label cursor-pointer gap-2">
      <input type="checkbox" class="checkbox checkbox-sm" bind:checked={fraFjor} />
      <span class="label-text">Fra {year - 1}</span>
    </label>
    <input class="input input-sm input-bordered w-24" placeholder="± %" bind:value={prosent} />
    <button class="btn btn-sm" onclick={nyttBudsjett}>Nytt budsjett {year}</button>
  </div>

  {#if aktivt && aktivt.status === "utkast"}
    <BudsjettRutenett {companyId} budsjett={aktivt} onDone={reload} />
  {/if}

  <Avvik av={data.av} {year} />
{/if}
