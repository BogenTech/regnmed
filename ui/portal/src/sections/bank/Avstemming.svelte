<script>
  // Bankavstemming: «umatchet» er ALLTID beregnet av hovedboken og
  // kontoutskriften — aldri lagret som tilstand. Motoren kobler bare
  // det den er sikker på; uavgjorte treff havner her til manuell
  // behandling, de gjettes aldri.
  import { post } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, recon, onDone } = $props();

  // Valgt hovedbokspost per banktransaksjon; tom = første i listen,
  // slik <select> selv står når ingenting er rørt.
  let valgt = $state({});

  async function koble(t) {
    const entryId = valgt[t.bank_transaction_id] || recon.unmatched_entries[0]?.entry_id;
    try {
      await post("/companies/" + companyId + "/bank/matches", {
        bank_transaction_id: t.bank_transaction_id,
        entry_id: entryId,
      });
      toast("Koblet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#if recon}
  <Card title={"Avstemming " + recon.account}>
    <div class="stats bg-base-200 mb-4">
      <div class="stat">
        <div class="stat-title">Hovedbok</div>
        <div class="stat-value text-lg">{kr(recon.ledger_balance_ore)}</div>
      </div>
      <div class="stat">
        <div class="stat-title">Bank ({recon.statement_to_date || ""})</div>
        <div class="stat-value text-lg">
          {recon.statement_closing_ore != null ? kr(recon.statement_closing_ore) : "–"}
        </div>
      </div>
      <div class="stat">
        <div class="stat-title">Koblet</div>
        <div class="stat-value text-lg">{recon.matched_count}</div>
      </div>
    </div>
    <h3 class="font-semibold mb-2">Ukoblede banktransaksjoner</h3>
    <table class="table table-sm">
      <tbody>
        {#each recon.unmatched_bank as t (t.bank_transaction_id)}
          <tr>
            <td>{t.booking_date}</td>
            <td>{t.description}</td>
            <td class="text-right">{kr(t.amount_ore)}</td>
            <td class="flex gap-1">
              <select
                class="select select-xs"
                bind:value={valgt[t.bank_transaction_id]}
              >
                {#each recon.unmatched_entries as e (e.entry_id)}
                  <option value={e.entry_id}>{e.voucher} {e.date} {kr(e.amount_ore)}</option>
                {/each}
              </select>
              <button class="btn btn-xs" onclick={() => koble(t)}>Koble</button>
            </td>
          </tr>
        {:else}
          <tr><td class="opacity-70">Ingen — avstemt!</td></tr>
        {/each}
      </tbody>
    </table>
  </Card>
{:else}
  <Card title="Avstemming">
    <p class="opacity-70">Ingen kontoutskrifter importert ennå.</p>
  </Card>
{/if}
