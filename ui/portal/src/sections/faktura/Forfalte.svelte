<script>
  import { kr } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import PurringForm from "./PurringForm.svelte";

  let { companyId, overdue, onDone } = $props();

  const STEG_NAVN = { paminnelse: "påminnelse", purring: "purring", inkassovarsel: "inkassovarsel" };
  const NESTE_STEG = { paminnelse: "purring", purring: "inkassovarsel", inkassovarsel: "inkassovarsel" };

  let purring = $state(null); // {invoiceId, steg}
</script>

<Card title="Forfalte fakturaer">
  <div class="flex gap-2 mb-2 text-sm">
    {#each ["1-14", "15-30", "30+"] as b}
      <span class="badge badge-ghost">{b} dager: {kr(overdue.buckets[b] || 0)}</span>
    {/each}
  </div>
  <table class="table table-sm">
    <thead>
      <tr>
        <th>Nr</th><th>Kunde</th><th>Forfall</th><th>Alder</th>
        <th class="text-right">Utestående</th><th>Siste skritt</th><th></th>
      </tr>
    </thead>
    <tbody>
      {#each overdue.invoices as i (i.invoice_id)}
        <tr>
          <td>{i.invoice_no}</td>
          <td>{i.party_name}</td>
          <td>{i.due_date}</td>
          <td>
            <span
              class="badge badge-sm {i.bucket === '30+'
                ? 'badge-error'
                : i.bucket === '15-30'
                  ? 'badge-warning'
                  : 'badge-ghost'}"
            >
              {i.days_overdue} dager
            </span>
          </td>
          <td class="text-right">{kr(i.remaining_ore)}</td>
          <td class="text-xs opacity-70">
            {i.last_steg ? STEG_NAVN[i.last_steg] + " " + i.last_sent : "–"}
          </td>
          <td>
            <button
              class="btn btn-xs btn-outline"
              onclick={() =>
                (purring = {
                  invoiceId: i.invoice_id,
                  steg: i.last_steg ? NESTE_STEG[i.last_steg] : "paminnelse",
                })}
            >
              Purring
            </button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if purring}
    {#key purring.invoiceId}
      <PurringForm
        {companyId}
        invoiceId={purring.invoiceId}
        suggestedSteg={purring.steg}
        {onDone}
      />
    {/key}
  {/if}
</Card>
