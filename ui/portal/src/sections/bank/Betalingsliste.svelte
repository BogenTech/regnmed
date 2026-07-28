<script>
  // Betalingsliste og remittering: opprettelse og godkjenning er to
  // SEPARATE handlinger (fire øyne), og statusene går bare én vei —
  // utkast → godkjent → utbetalt (utkast kan annulleres).
  import { post } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import Card from "../../components/Card.svelte";

  let { companyId, payable, runs, onDone } = $props();

  const STATUS_BADGE = {
    utkast: "badge-warning",
    godkjent: "badge-info",
    utbetalt: "badge-success",
    annullert: "badge-ghost",
  };

  async function lagListe() {
    const betalbare = payable.filter((p) => !p.i_kjoring && p.bank_account);
    if (!betalbare.length) {
      toast("Ingen betalbare poster (mangler kontonummer?)", false);
      return;
    }
    try {
      await post("/companies/" + companyId + "/payments/runs", {
        items: betalbare.map((p) => ({ entry_id: p.entry_id })),
      });
      toast("Betalingsliste laget (" + betalbare.length + " poster)", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function godkjenn(r) {
    try {
      await post("/companies/" + companyId + "/payments/runs/" + r.run_id + "/approve", {});
      toast("Godkjent — pain.001 klar for nedlasting", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function annuller(r) {
    try {
      await post("/companies/" + companyId + "/payments/runs/" + r.run_id + "/cancel", {});
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function utbetalt(r) {
    try {
      const result = await post(
        "/companies/" + companyId + "/payments/runs/" + r.run_id + "/settle",
        {},
      );
      toast("Utbetaling bokført som bilag " + result.voucher, true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  function fil(r) {
    download(
      "/companies/" + companyId + "/payments/runs/" + r.run_id + "/file",
      "pain001-" + r.run_id + ".xml",
    );
  }
</script>

<Card title="Betalingsliste og remittering">
  <p class="text-sm opacity-70 mb-2">
    Åpne leverandørposter samles i en betalingsliste; godkjenning er en egen handling som lager
    pain.001-filen (lastes opp i nettbanken). «Registrer utbetalt» bokfører utbetalingen og
    lukker postene i reskontroen — bankimporten kobler seg mot det bilaget. Med attestering aktiv
    må godkjenneren være en annen enn den som laget listen.
  </p>
  {#if payable.length}
    <h3 class="font-semibold mb-1">Åpne leverandørposter</h3>
    <table class="table table-sm mb-2">
      <thead>
        <tr>
          <th>Bilag</th><th>Dato</th><th>Leverandør</th><th>Tekst</th>
          <th class="text-right">Beløp</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each payable as p (p.entry_id)}
          <tr class={p.i_kjoring ? "opacity-50" : ""}>
            <td>{p.voucher}</td>
            <td>{p.date}</td>
            <td>
              {p.party_name}
              {#if !p.bank_account}
                <span
                  class="badge badge-warning badge-xs"
                  title="Mangler kontonummer — sett det på leverandørsiden under Reskontro"
                >
                  mangler konto
                </span>
              {/if}
            </td>
            <td>{p.description || ""}</td>
            <td class="text-right">{kr(p.belop_ore)}</td>
            <td>
              {#if p.i_kjoring}
                <span class="badge badge-ghost badge-xs">i kjøring</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
    <button class="btn btn-sm btn-outline mb-3" onclick={lagListe}>
      Lag betalingsliste av betalbare poster
    </button>
  {:else}
    <p class="opacity-70 text-sm mb-2">Ingen åpne leverandørposter.</p>
  {/if}
  {#if runs.length}
    <h3 class="font-semibold mb-1">Kjøringer</h3>
    <table class="table table-sm">
      <thead>
        <tr>
          <th>Utførelse</th><th class="text-right">Poster</th><th class="text-right">Sum</th>
          <th>Status</th><th>Laget / godkjent av</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each runs as r (r.run_id)}
          <tr>
            <td>{r.execution_date}</td>
            <td class="text-right">{r.antall}</td>
            <td class="text-right">{kr(r.sum_ore)}</td>
            <td>
              <span class="badge badge-sm {STATUS_BADGE[r.status] || 'badge-ghost'}">
                {r.status}
              </span>
            </td>
            <td class="text-xs opacity-70">
              {r.created_by}{r.approved_by ? " / " + r.approved_by : ""}
            </td>
            <td>
              {#if r.status === "utkast"}
                <button class="btn btn-xs btn-outline" onclick={() => godkjenn(r)}>Godkjenn</button>
                <button class="btn btn-xs btn-ghost" onclick={() => annuller(r)}>Annuller</button>
              {:else if r.status === "godkjent"}
                <button class="btn btn-xs btn-ghost" onclick={() => fil(r)}>pain.001</button>
                <button class="btn btn-xs btn-outline" onclick={() => utbetalt(r)}>
                  Registrer utbetalt
                </button>
              {:else if r.status === "utbetalt"}
                <button class="btn btn-xs btn-ghost" onclick={() => fil(r)}>pain.001</button>
                {#if r.settled_voucher}
                  <span class="text-xs opacity-60">bilag {r.settled_voucher}</span>
                {/if}
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</Card>
