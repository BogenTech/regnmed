<script>
  // Kunde-/leverandørspesifikasjon (bokføringsforskriften §3-1 nr. 3–4):
  // én blokk per part med inngående saldo, posteringene i perioden med
  // løpende saldo og bilagshenvisning, og utgående saldo. Én fane for
  // begge — valget kunde/leverandør bytter bare endepunkt.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, from, to } = $props();

  let kind = $state("kunde");
  let data = $state(null);

  $effect(() => {
    data = null;
    const rapport = kind === "kunde" ? "kundespesifikasjon" : "leverandorspesifikasjon";
    api("/companies/" + companyId + "/reports/" + rapport + "?from=" + from + "&to=" + to)
      .then((svar) => (data = svar))
      .catch((error) => toast(error.message, false));
  });
</script>

<div class="join mb-3">
  <button
    class="join-item btn btn-sm {kind === 'kunde' ? 'btn-active' : ''}"
    onclick={() => (kind = "kunde")}
  >
    Kunder
  </button>
  <button
    class="join-item btn btn-sm {kind === 'leverandor' ? 'btn-active' : ''}"
    onclick={() => (kind = "leverandor")}
  >
    Leverandører
  </button>
</div>

{#if !data}
  <span class="loading loading-spinner loading-sm"></span>
{:else if !data.parties.length}
  <p class="opacity-60">Ingen {kind === "kunde" ? "kunder" : "leverandører"} med saldo eller bevegelse i perioden.</p>
{:else}
  {#each data.parties as p (p.party_no)}
    <div class="mb-4">
      <div class="flex justify-between items-baseline">
        <h4 class="font-semibold">{p.party_no} {p.name}</h4>
        <span class="text-sm opacity-70">Inngående {kr(p.inngaende_ore)}</span>
      </div>
      {#if p.posts.length}
        <table class="table table-sm">
          <thead>
            <tr>
              <th>Bilag</th><th>Dato</th><th>Konto</th><th>Tekst</th>
              <th class="text-right">Beløp</th><th class="text-right">Saldo</th>
            </tr>
          </thead>
          <tbody>
            {#each p.posts as post}
              <tr>
                <td>{post.bilag}</td>
                <td>{post.date}</td>
                <td>{post.account}</td>
                <td>{post.description}</td>
                <td class="text-right">{kr(post.amount_ore)}</td>
                <td class="text-right">{kr(post.saldo_ore)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <p class="text-sm opacity-60">Ingen bevegelse i perioden.</p>
      {/if}
      <div class="text-right text-sm font-semibold">Utgående {kr(p.utgaende_ore)}</div>
    </div>
  {/each}
{/if}
