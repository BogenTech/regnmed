<script>
  // Én fane av kravene, sidevis fra serveren (50 om gangen). Tellerne i
  // fanene kommer i samme svar, så fanene og listen kan aldri spriker.
  import { api } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { STATUS } from "./expenseMath.js";

  let { companyId, tab, tabs, seesAll } = $props();

  const PAGE = 50;
  let page = $state(null); // { expenses, total, counts }
  let offset = $state(0);
  let error = $state(null);

  $effect(() => {
    const status = tab || "apne";
    const at = offset;
    api("/companies/" + companyId + "/expenses?status=" + status + "&limit=" + PAGE + "&offset=" + at)
      .then((r) => (page = r))
      .catch((e) => (error = e.message));
  });

  const count = (key, c) =>
    !c ? null : key === "" ? c.innsendt + c.godkjent : c[key];

  const EMPTY = {
    "": seesAll
      ? "Ingenting venter på godkjenning eller utbetaling."
      : "Du har ingen åpne krav. Utlegg du sender inn ligger her til de er utbetalt.",
    utbetalt: "Ingen krav er utbetalt ennå.",
    avvist: "Ingen krav er avvist.",
  };
</script>

<nav class="tabs tabs-box mb-4 w-fit max-w-full" aria-label="Utlegg">
  {#each tabs as [key, label] (key)}
    {@const n = count(key, page?.counts)}
    <a
      class="tab whitespace-nowrap {tab === key ? 'tab-active' : ''}"
      href={"#/c/" + companyId + "/utlegg" + (key ? "/" + key : "")}
      aria-current={tab === key ? "page" : undefined}
    >
      {label}{#if n}&nbsp;<span class="badge badge-sm">{n}</span>{/if}
    </a>
  {/each}
</nav>

<div class="card border border-base-300 bg-base-100">
  <div class="card-body p-0 sm:p-2 overflow-x-auto">
    {#if error}
      <div class="alert alert-error m-4" role="alert"><span>{error}</span></div>
    {:else if page === null}
      <div class="flex justify-center p-8"><span class="loading loading-spinner"></span></div>
    {:else if page.expenses.length === 0}
      <p class="opacity-70 p-4">{EMPTY[tab]}</p>
    {:else}
      <table class="table">
        <caption class="sr-only">Utleggskrav. Velg et krav for å se kvitteringen og hva som skjer videre.</caption>
        <thead>
          <tr>
            <th scope="col">Krav</th>
            <th scope="col" class="hidden lg:table-cell">Dato</th>
            {#if seesAll}<th scope="col" class="hidden xl:table-cell">Hvem</th>{/if}
            <th scope="col" class="hidden lg:table-cell">Status</th>
            <th scope="col" class="text-right">Beløp</th>
          </tr>
        </thead>
        <tbody>
          {#each page.expenses as e (e.expense_id)}
            {@const s = STATUS[e.status]}
            <tr class="relative hover:bg-base-200 focus-within:bg-base-200">
              <td class="min-w-0">
                <a class="font-medium break-words after:absolute after:inset-0" href={"#/c/" + companyId + "/utlegg/" + e.expense_id}>
                  {e.beskrivelse}
                </a>
                <span class="block text-sm opacity-70">
                  {e.kind === "kjoring" ? "Kjøring, " + e.km + " km" : "Utlegg"}<span class="lg:hidden">, {formatDate(e.dato)}</span>{#if seesAll}<span class="xl:hidden">, {e.person}</span>{/if}
                </span>
                <span class="badge badge-sm badge-soft {s?.badge} lg:hidden mt-1">{s?.text || e.status}</span>
              </td>
              <td class="hidden lg:table-cell whitespace-nowrap">{formatDate(e.dato)}</td>
              {#if seesAll}<td class="hidden xl:table-cell">{e.person}</td>{/if}
              <td class="hidden lg:table-cell">
                <span class="badge badge-sm badge-soft {s?.badge} whitespace-nowrap">{s?.text || e.status}</span>
              </td>
              <td class="text-right tabular-nums whitespace-nowrap">{kr(e.belop_ore)} kr</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if page.total > PAGE}
        <div class="flex flex-wrap items-center gap-2 p-4">
          <span class="text-sm opacity-70 mr-auto">
            Viser {offset + 1}–{offset + page.expenses.length} av {page.total}
          </span>
          <button class="btn btn-sm" disabled={offset === 0} onclick={() => (offset = Math.max(0, offset - PAGE))}>Forrige</button>
          <button class="btn btn-sm" disabled={offset + PAGE >= page.total} onclick={() => (offset += PAGE)}>Neste</button>
        </div>
      {/if}
    {/if}
  </div>
</div>
