<script>
  // Kravene og de tre skrittene. Overgangene er ENVEIS (innsendt →
  // godkjent/avvist → utbetalt), så hver rad viser bare skrittene som
  // finnes fra dens egen status — og avvisning krever begrunnelse.
  import { post } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { skjema, sporsmal } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import Card from "../../components/Card.svelte";

  let { companyId, expenses, onDone } = $props();

  const STATUS_BADGE = {
    innsendt: "badge-warning",
    godkjent: "badge-info",
    avvist: "badge-error",
    utbetalt: "badge-success",
  };

  async function godkjenn(e) {
    const isKjoring = e.kind === "kjoring";
    const felter = [
      { navn: "konto", etikett: "Kostnadskonto", standard: isKjoring ? "7100" : "7790" },
    ];
    if (!isKjoring) {
      felter.push({
        navn: "mva",
        etikett: "Mva-kode for inngående mva",
        valgfri: true,
        hjelp: "Tom = ingen",
      });
    }
    const svar = await skjema("Godkjenn krav", felter, { ok: "Godkjenn" });
    if (!svar) return;
    const body = { konto: svar.konto };
    if (!isKjoring && svar.mva) body.mva_kode = svar.mva;
    try {
      const result = await post(
        "/companies/" + companyId + "/expenses/" + e.expense_id + "/approve",
        body,
      );
      toast("Godkjent — bilag " + result.voucher + (result.warning ? " — " + result.warning : ""), true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function avvis(e) {
    const note = await sporsmal("Begrunnelse for avvisning (påkrevd):", {
      type: "textarea",
      ok: "Avvis",
      farlig: true,
    });
    if (!note) return;
    try {
      await post("/companies/" + companyId + "/expenses/" + e.expense_id + "/reject", {
        note,
      });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function utbetal(e) {
    try {
      const result = await post(
        "/companies/" + companyId + "/expenses/" + e.expense_id + "/pay",
        {},
      );
      toast("Utbetalt — bilag " + result.voucher, true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Krav">
  {#if expenses.length}
    <table class="table table-sm">
      <thead>
        <tr>
          <th>Dato</th><th>Hvem</th><th>Type</th><th>Beskrivelse</th>
          <th class="text-right">Beløp</th><th>Status</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each expenses as e (e.expense_id)}
          <tr>
            <td>{e.dato}</td>
            <td>{e.person}</td>
            <td>{e.kind === "kjoring" ? "kjøring" : "utlegg"}</td>
            <td>
              {e.beskrivelse}
              <span class="opacity-70">
                {#if e.kind === "kjoring"}
                  {e.km} km à {kr(e.sats_ore_per_km)}
                  <!-- Trekkpliktig del er et TYDELIG VARSEL, aldri skjult:
                       den skal lønnsinnberettes, og a-meldingen finnes ikke ennå. -->
                  {#if e.trekkpliktig_ore > 0}
                    <span
                      class="badge badge-warning badge-xs"
                      title="Skal lønnsinnberettes — a-melding er ikke støttet ennå"
                    >
                      trekkpliktig {kr(e.trekkpliktig_ore)}
                    </span>
                  {/if}
                {:else if e.receipt_filename}
                  <button
                    class="link"
                    onclick={() =>
                      download(
                        "/companies/" + companyId + "/expenses/" + e.expense_id + "/receipt",
                        e.receipt_filename,
                      )}
                  >
                    {e.receipt_filename}
                  </button>
                {/if}
              </span>
            </td>
            <td class="text-right">{kr(e.belop_ore)}</td>
            <td>
              <span class="badge badge-sm {STATUS_BADGE[e.status] || 'badge-ghost'}">
                {e.status}
              </span>
              {#if e.voucher}<span class="text-xs opacity-60">{e.voucher}</span>{/if}
            </td>
            <td>
              {#if e.status === "innsendt"}
                <button class="btn btn-xs btn-outline" onclick={() => godkjenn(e)}>Godkjenn</button>
                <button class="btn btn-xs btn-ghost" onclick={() => avvis(e)}>Avvis</button>
              {:else if e.status === "godkjent"}
                <button class="btn btn-xs btn-outline" onclick={() => utbetal(e)}>Utbetal</button>
              {:else if e.status === "avvist" && e.avvist_note}
                <span class="text-xs opacity-70" title={e.avvist_note}>{e.avvist_note}</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="opacity-70">Ingen krav ennå.</p>
  {/if}
</Card>
