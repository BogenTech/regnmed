<script>
  import { post } from "../../lib/api.js";
  import { kr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import { sendDocument } from "../../lib/utsendelse.js";
  import Card from "../../components/Card.svelte";

  let { companyId, invoices, onDone } = $props();

  async function kreditnota(invoice) {
    if (!confirm("Opprette kreditnota for hele fakturaen?")) return;
    try {
      await post("/companies/" + companyId + "/invoices/" + invoice.invoice_id + "/credit-note", {});
      toast("Kreditnota opprettet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function gjenta(invoice) {
    const intervall = prompt("Intervall (manedlig, kvartalsvis, arlig):", "manedlig");
    if (!intervall) return;
    const neste = prompt("Første genereringsdato (ÅÅÅÅ-MM-DD):", today());
    if (!neste) return;
    try {
      await post("/companies/" + companyId + "/invoice-templates", {
        from_invoice_id: invoice.invoice_id,
        intervall: intervall.trim(),
        neste_dato: neste.trim(),
      });
      toast("Repeterende mal opprettet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Fakturaer">
  <table class="table table-sm">
    <thead>
      <tr>
        <th>Nr</th><th>Kunde</th><th>Dato</th><th>KID</th>
        <th class="text-right">Beløp</th><th class="text-right">Utestående</th><th></th>
      </tr>
    </thead>
    <tbody>
      {#each invoices as i (i.invoice_id)}
        <tr>
          <td>{i.invoice_no}</td>
          <td>{i.party_name}</td>
          <td>{i.invoice_date}</td>
          <td class="font-mono">{i.kid}</td>
          <td class="text-right">{kr(i.gross_ore)}</td>
          <td class="text-right">{kr(i.remaining_ore)}</td>
          <td>
            <button
              class="btn btn-xs btn-ghost"
              onclick={() =>
                download(
                  "/companies/" + companyId + "/invoices/" + i.invoice_id + "/pdf",
                  (i.is_credit_note ? "kreditnota" : "faktura") + "-" + i.invoice_no + ".pdf",
                )}
            >
              PDF
            </button>
            <button
              class="btn btn-xs btn-ghost"
              title="EHF (PEPPOL BIS Billing 3.0)"
              onclick={() =>
                download(
                  "/companies/" + companyId + "/invoices/" + i.invoice_id + "/ehf",
                  "ehf-" + i.invoice_no + ".xml",
                )}
            >
              EHF
            </button>
            <button
              class="btn btn-xs btn-ghost"
              onclick={() =>
                sendDocument("/companies/" + companyId + "/invoices/" + i.invoice_id + "/send")}
            >
              Send
            </button>
            {#if !i.is_credit_note}
              <button
                class="btn btn-xs btn-ghost"
                title="Opprett repeterende mal fra denne"
                onclick={() => gjenta(i)}
              >
                Gjenta
              </button>
            {/if}
            {#if !i.is_credit_note && i.remaining_ore !== 0}
              <button class="btn btn-xs btn-outline" onclick={() => kreditnota(i)}>
                Kreditnota
              </button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</Card>
