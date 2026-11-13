<script>
  // Budsjett og avvik (#41): budsjettet er den eneste MENINGEN i
  // systemet. Et utkast er fritt redigerbart; fastsettelse fryser
  // versjonen, og en revisjon blir en NY versjon — derfor kan
  // avviksrapporten alltid navngi planen den måler mot.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { kr } from "../../lib/format.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import LoadError from "../../components/LoadError.svelte";
  import BudsjettRutenett from "./BudsjettRutenett.svelte";
  import Avvik from "./Avvik.svelte";

  let { companyId, year } = $props();

  let valgtBudsjett = $state("");
  let data = $state(null);
  let error = $state(null);
  let seq = 0;

  let navn = $state("");
  let fraFjor = $state(false);
  let prosent = $state("0");

  // `fresh` keeps the page on screen while a save reloads it.
  function load(fresh = true) {
    const mine = ++seq;
    if (fresh) data = null;
    error = null;
    Promise.all([
      api("/companies/" + companyId + "/budgets?year=" + year),
      api(
        "/companies/" + companyId + "/reports/avvik?year=" + year +
          (valgtBudsjett ? "&budget_id=" + valgtBudsjett : ""),
      ),
    ])
      .then(([budsjetter, av]) => mine === seq && (data = { budsjetter: budsjetter.budgets, av }))
      .catch((e) => mine === seq && (error = e.message));
  }

  const reload = () => load(false);

  $effect(() => {
    load();
  });

  const STATUS = { utkast: "Utkast", fastsatt: "Fastsatt" };

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
    const ok = await bekreft(
      "Et fastsatt budsjett kan ikke endres. Vil du revidere det senere, lager du en ny versjon.",
      { tittel: "Fastsett " + b.navn + " v" + b.versjon + "?", ok: "Fastsett" },
    );
    if (!ok) return;
    try {
      await post("/companies/" + companyId + "/budgets/" + b.budget_id + "/fastsett", {});
      toast("Budsjettet er fastsatt", true);
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function forkast(b) {
    const ok = await bekreft("Utkastet og alle linjene i det slettes. Dette kan ikke angres.", {
      tittel: "Forkast " + b.navn + " v" + b.versjon + "?",
      ok: "Forkast",
      farlig: true,
    });
    if (!ok) return;
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

{#if error}
  <LoadError {error} retry={() => load()} />
{:else if !data}
  <span class="loading loading-spinner loading-sm" role="status" aria-label="Henter budsjettene"></span>
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
          <tr
            class={aktivt && b.budget_id === aktivt.budget_id ? "bg-base-200" : ""}
            aria-current={aktivt && b.budget_id === aktivt.budget_id ? "true" : undefined}
          >
            <td>v{b.versjon}</td>
            <td>{b.navn}</td>
            <td>
              <span class="badge badge-sm {b.status === 'fastsatt' ? 'badge-success' : 'badge-warning'}">
                {STATUS[b.status] || b.status}
              </span>
            </td>
            <td class="text-right tabular-nums whitespace-nowrap">{kr(b.sum_ore)}</td>
            <td class="text-xs opacity-70">
              {b.created_by}{b.fastsatt_by ? " → " + b.fastsatt_by : ""}
            </td>
            <td class="whitespace-nowrap">
              {#if !aktivt || b.budget_id !== aktivt.budget_id}
                <button
                  class="btn btn-xs btn-ghost"
                  aria-label={"Vis " + b.navn + " v" + b.versjon}
                  onclick={() => (valgtBudsjett = b.budget_id)}
                >
                  Vis
                </button>
              {/if}
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

  <form
    class="flex gap-2 items-end flex-wrap"
    onsubmit={(e) => {
      e.preventDefault();
      nyttBudsjett();
    }}
  >
    <label class="flex flex-col gap-1 text-sm">
      Navn (valgfritt)
      <input class="input input-sm w-48" bind:value={navn} />
    </label>
    <label class="label cursor-pointer gap-2 text-sm">
      <input type="checkbox" class="checkbox checkbox-sm" bind:checked={fraFjor} />
      <span>Start fra regnskapet for {year - 1}</span>
    </label>
    {#if fraFjor}
      <label class="flex flex-col gap-1 text-sm">
        Juster med %
        <input class="input input-sm w-24" inputmode="decimal" placeholder="f.eks. 3,5" bind:value={prosent} />
      </label>
    {/if}
    <button type="submit" class="btn btn-sm">Nytt budsjett {year}</button>
  </form>

  {#if aktivt && aktivt.status === "utkast"}
    <BudsjettRutenett {companyId} budsjett={aktivt} onDone={reload} />
  {/if}

  <Avvik av={data.av} {year} />
{/if}
