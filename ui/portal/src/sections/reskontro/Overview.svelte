<script>
  // The front page: does the reskontro agree with the hovedbok (the
  // revisjonsrapport's check 4, same numbers), and which parties have
  // something to match — open items on both sides.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";

  let { companyId } = $props();

  let data = $state(null);
  let error = $state(null);

  $effect(() => {
    api("/companies/" + companyId + "/reskontro/overview")
      .then((r) => (data = r))
      .catch((e) => (error = e.message));
  });

  const KIND = { kunde: "Kunder", leverandor: "Leverandører" };
  const ok = (a) => a.ledger_ore === a.subledger_ore && a.without_party_count === 0 && a.wrong_kind_count === 0;
  const partyLink = (p) =>
    "#/c/" + companyId + "/" + (p.kind === "leverandor" ? "leverandorer" : "kunder") + "/" + p.party_id + "?avstem";
</script>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if data === null}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else}
  <section class="mb-6" aria-labelledby="stemmer-tittel">
    <h2 id="stemmer-tittel" class="text-lg font-semibold mb-2">Reskontro mot hovedbok</h2>
    {#if data.accounts.length === 0}
      <div class="alert alert-info" role="status">
        <span>
          Ingen konto er reskontrokonto ennå, så ingen poster kan føres på kunder eller leverandører.
          <a class="link" href={"#/c/" + companyId + "/reskontro/innstillinger"}>Velg reskontrokontoene</a>.
        </span>
      </div>
    {:else}
      <ul class="grid gap-2 sm:grid-cols-2">
        {#each data.accounts as a (a.account)}
          {@const good = ok(a)}
          <li class="card border bg-base-100 {good ? 'border-base-300' : 'border-warning'}">
            <div class="card-body p-4 gap-1">
              <div class="flex items-baseline gap-2">
                <span class="font-semibold mr-auto">{a.account} {a.name || ""}</span>
                <span class="badge badge-sm badge-soft {good ? 'badge-success' : 'badge-warning'}">{good ? "Stemmer" : "Avvik"}</span>
              </div>
              <p class="text-sm opacity-70">{a.reskontro_kind ? KIND[a.reskontro_kind] : "Ikke reskontrokonto, men har poster med part"}</p>
              <dl class="grid grid-cols-2 gap-x-4 tabular-nums text-sm">
                <dt class="opacity-70">Hovedbok</dt><dd class="text-right">{kr(a.ledger_ore)} kr</dd>
                <dt class="opacity-70">Sum parter</dt><dd class="text-right">{kr(a.subledger_ore)} kr</dd>
              </dl>
              {#if !good}
                <p class="text-sm">
                  {#if a.without_party_count}{a.without_party_count} {a.without_party_count === 1 ? "post" : "poster"} uten part. {/if}
                  {#if a.wrong_kind_count}{a.wrong_kind_count} {a.wrong_kind_count === 1 ? "post" : "poster"} med feil type part. {/if}
                  {#if a.ledger_ore !== a.subledger_ore}Differanse {kr(a.ledger_ore - a.subledger_ore)} kr.{/if}
                </p>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section aria-labelledby="avstem-tittel">
    <h2 id="avstem-tittel" class="text-lg font-semibold mb-2">
      {data.to_match_total === 0
        ? "Ingenting å avstemme"
        : data.to_match_total === 1
          ? "1 part har poster å avstemme"
          : data.to_match_total + " parter har poster å avstemme"}
    </h2>
    {#if data.to_match_total === 0}
      <p class="opacity-70">Ingen kunde eller leverandør har åpne poster på begge sider. Innbetalinger og fakturaer som hører sammen er avstemt.</p>
    {:else}
      <div class="card border border-base-300 bg-base-100">
        <div class="card-body p-0 sm:p-2 overflow-x-auto">
          <table class="table">
            <caption class="sr-only">Parter med åpne poster på begge sider. Velg en for å avstemme.</caption>
            <thead>
              <tr>
                <th scope="col">Part</th>
                <th scope="col" class="text-right">Å avstemme</th>
              </tr>
            </thead>
            <tbody>
              {#each data.to_match as p (p.party_id)}
                <tr class="relative hover:bg-base-200 focus-within:bg-base-200">
                  <td>
                    <a class="font-medium after:absolute after:inset-0" href={partyLink(p)}>{p.name}</a>
                    <span class="block text-sm opacity-70">{p.kind === "leverandor" ? "Leverandør" : "Kunde"} nr {p.party_no}</span>
                  </td>
                  <td class="text-right tabular-nums text-sm">
                    <!-- The smaller side is what can be matched right away. -->
                    <span class="whitespace-nowrap font-medium">{kr(Math.min(p.open_positive_ore, -p.open_negative_ore))} kr</span>
                    <span class="block opacity-70">
                      {p.kind === "leverandor"
                        ? kr(-p.open_negative_ore) + " i fakturaer, " + kr(p.open_positive_ore) + " i betalinger"
                        : kr(p.open_positive_ore) + " i fakturaer, " + kr(-p.open_negative_ore) + " innbetalt"}
                    </span>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if data.to_match_total > data.to_match.length}
            <p class="text-sm opacity-70 p-4">Viser {data.to_match.length} av {data.to_match_total}, alfabetisk.</p>
          {/if}
        </div>
      </div>
    {/if}
  </section>
{/if}
