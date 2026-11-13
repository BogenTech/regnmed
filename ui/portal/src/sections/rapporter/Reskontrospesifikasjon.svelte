<script>
  // Kunde-/leverandørspesifikasjon (bokføringsforskriften §3-1 nr. 3–4):
  // én blokk per part med inngående saldo, posteringene i perioden med
  // løpende saldo og bilagshenvisning, og utgående saldo. Én fane for
  // begge — valget kunde/leverandør bytter bare endepunkt.
  import { api } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import LoadError from "../../components/LoadError.svelte";

  let { companyId, from, to, params } = $props();

  let kind = $derived(params.kind === "leverandor" ? "leverandor" : "kunde");
  let data = $state(null);
  let error = $state(null);
  let seq = 0;

  function load() {
    const mine = ++seq;
    const rapport = kind === "kunde" ? "kundespesifikasjon" : "leverandorspesifikasjon";
    data = null;
    error = null;
    api("/companies/" + companyId + "/reports/" + rapport + "?from=" + from + "&to=" + to)
      .then((r) => mine === seq && (data = r))
      .catch((e) => mine === seq && (error = e.message));
  }
  $effect(load);

  const num = "text-right tabular-nums whitespace-nowrap";
</script>

<div class="join mb-3" role="group" aria-label="Spesifikasjon for">
  <button
    class="join-item btn btn-sm {kind === 'kunde' ? 'btn-active' : ''}"
    aria-pressed={kind === "kunde"}
    onclick={() => (params.kind = undefined)}
  >
    Kunder
  </button>
  <button
    class="join-item btn btn-sm {kind === 'leverandor' ? 'btn-active' : ''}"
    aria-pressed={kind === "leverandor"}
    onclick={() => (params.kind = "leverandor")}
  >
    Leverandører
  </button>
</div>

{#if error}
  <LoadError {error} retry={load} />
{:else if !data}
  <span class="loading loading-spinner loading-sm" role="status" aria-label="Henter spesifikasjonen"></span>
{:else if !data.parties.length}
  <p class="opacity-60">Ingen {kind === "kunde" ? "kunder" : "leverandører"} med saldo eller bevegelse i perioden.</p>
{:else}
  {#each data.parties as p (p.party_no)}
    <section class="mb-4" aria-labelledby={"part-" + p.party_no}>
      <div class="flex justify-between items-baseline flex-wrap gap-x-2">
        <h3 id={"part-" + p.party_no} class="font-semibold">{p.party_no} {p.name}</h3>
        <span class="text-sm opacity-70 whitespace-nowrap">Inngående {kr(p.inngaende_ore)}</span>
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
            {#each p.posts as post, i (i)}
              <tr>
                <td class="whitespace-nowrap">
                  <a class="link link-hover font-medium" href={"#/c/" + companyId + "/hovedbok/" + post.voucher_id}>{post.bilag}</a>
                </td>
                <td class="whitespace-nowrap">{formatDate(post.date)}</td>
                <td>{post.account}</td>
                <td>{post.description}</td>
                <td class={num}>{kr(post.amount_ore)}</td>
                <td class={num}>{kr(post.saldo_ore)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <p class="text-sm opacity-60">Ingen bevegelse i perioden.</p>
      {/if}
      <div class="text-right text-sm font-semibold whitespace-nowrap">Utgående {kr(p.utgaende_ore)}</div>
    </section>
  {/each}
{/if}
