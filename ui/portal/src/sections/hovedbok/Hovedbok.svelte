<script>
  // Hovedbok (docs/hovedbok.md): the book first — every voucher with its
  // lines — and each voucher on its own page (#/c/{id}/hovedbok/{uuid}),
  // where it is reversed or given its missing document. Manual posting,
  // the chart of accounts, accruals and the daily cash settlement each
  // have their own tab and address. A four-digit `extra` is an account's
  // postings for a year.
  import VoucherList from "./VoucherList.svelte";
  import VoucherPage from "./VoucherPage.svelte";
  import NewVoucher from "./NewVoucher.svelte";
  import ChartOfAccounts from "./ChartOfAccounts.svelte";
  import AccountDetail from "./AccountDetail.svelte";
  import Accruals from "./Accruals.svelte";
  import Kassaoppgjor from "./Kassaoppgjor.svelte";

  let { companyId, extra, query = "" } = $props();

  const TABS = [
    ["", "Bilag"],
    ["ny", "Nytt bilag"],
    ["kontoplan", "Kontoplan"],
    ["periodisering", "Periodisering"],
    ["kasse", "Kassaoppgjør"],
  ];
  const isId = (s) => /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(s || "");
  const isAccount = (s) => /^\d{4}$/.test(s || "");
  let tab = $derived(TABS.some(([k]) => k === (extra || "")) ? extra || "" : null);
</script>

{#if isId(extra)}
  {#key companyId + extra}<VoucherPage {companyId} voucherId={extra} />{/key}
{:else if isAccount(extra)}
  {#key companyId + extra}<AccountDetail {companyId} account={extra} {query} />{/key}
{:else}
  <h1 class="text-2xl font-bold mb-4">Hovedbok</h1>
  {#if tab === null}
    <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
  {:else}
    <nav class="tabs tabs-box mb-4 w-fit max-w-full overflow-x-auto flex-nowrap" aria-label="Hovedbok">
      {#each TABS as [key, label] (key)}
        <a
          class="tab whitespace-nowrap {tab === key ? 'tab-active' : ''}"
          href={"#/c/" + companyId + "/hovedbok" + (key ? "/" + key : "")}
          aria-current={tab === key ? "page" : undefined}
        >
          {label}
        </a>
      {/each}
    </nav>
    {#key companyId + tab}
      {#if tab === ""}
        <VoucherList {companyId} {query} />
      {:else if tab === "ny"}
        <NewVoucher {companyId} />
      {:else if tab === "kontoplan"}
        <ChartOfAccounts {companyId} />
      {:else if tab === "periodisering"}
        <Accruals {companyId} />
      {:else}
        <Kassaoppgjor {companyId} />
      {/if}
    {/key}
  {/if}
{/if}
