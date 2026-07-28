<script>
  // Migrering inn i et TOMT selskap (#18/#19, docs/migration.md):
  // SAF-T-import m/ kontoplan-wizard, eller manuell åpningsbalanse.
  // Vises bare når hovedboken er tom.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, onDone } = $props();

  let xml = $state(null);
  let analysis = $state(null);
  let mapping = $state({}); // account_id -> NS 4102-nummer (bind per rad)
  let missing = $state({}); // account_id -> true når feltet sto tomt

  let obDate = $state(new Date().getFullYear() + "-01-01");
  let obLines = $state([
    { account: "", amount: "" },
    { account: "", amount: "" },
  ]);

  function importDone(result) {
    toast(
      result.vouchers +
        " bilag, " +
        result.accounts +
        " kontoer importert" +
        (result.warnings.length ? " (" + result.warnings.length + " merknader)" : ""),
      true,
    );
    onDone();
  }

  async function fileChosen(event) {
    const file = event.target.files[0];
    if (!file) return;
    xml = await file.text();
    try {
      analysis = null;
      const svar = await api("/companies/" + companyId + "/import/saft/analyze", {
        method: "POST",
        body: xml,
      });
      if (!svar.needs_mapping) {
        importDone(await api("/companies/" + companyId + "/import/saft", { method: "POST", body: xml }));
        return;
      }
      // Kontoplan-wizard: forslagene er heuristikk — admin bestemmer.
      mapping = Object.fromEntries(svar.accounts.map((a) => [a.account_id, a.suggested || ""]));
      missing = {};
      analysis = svar;
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function importMapped() {
    const endringer = {};
    missing = {};
    for (const a of analysis.accounts) {
      const value = (mapping[a.account_id] || "").trim();
      if (!value) {
        missing[a.account_id] = true;
        continue;
      }
      if (value !== a.account_id) endringer[a.account_id] = value;
    }
    if (Object.keys(missing).length) {
      toast("alle kontoer må ha et NS 4102-nummer", false);
      return;
    }
    try {
      importDone(
        await api("/companies/" + companyId + "/import/saft", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ file: xml, mapping: endringer }),
        }),
      );
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function postOpeningBalance() {
    const lines = obLines
      .filter((l) => l.account.trim() && l.amount.trim())
      .map((l) => ({
        account: l.account.trim(),
        amount_ore: Math.round(Number(l.amount.trim().replace(/\s/g, "").replace(",", ".")) * 100),
      }));
    try {
      const result = await post("/companies/" + companyId + "/opening-balance", {
        date: obDate,
        lines,
      });
      toast("Åpningsbalanse bokført som bilag " + result.voucher, true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Kom fra et annet system?">
  <p class="text-sm opacity-70 mb-2">
    Last opp en SAF-T-eksport (alle norske systemer kan lage en) — kontoplan,
    kunder/leverandører og hele historikken importeres i én operasjon. Har det gamle systemet en
    annen kontoplan, foreslår vi mapping til NS 4102 som du godkjenner først.
  </p>
  <input type="file" class="file-input file-input-bordered" accept=".xml" onchange={fileChosen} />
  {#if analysis}
    <div class="border border-base-300 rounded-lg p-3 mt-3">
      <p class="text-sm font-semibold mb-1">Kontoplanen må mappes til NS 4102</p>
      <p class="text-xs opacity-70 mb-2">
        {analysis.transactions} transaksjoner, {analysis.customers} kunder,
        {analysis.suppliers} leverandører. Forslagene under er heuristikk — du bestemmer.
      </p>
      <table class="table table-xs">
        <thead>
          <tr><th>Konto i filen</th><th>Navn</th><th>NS 4102</th><th>Forslag</th></tr>
        </thead>
        <tbody>
          {#each analysis.accounts as a (a.account_id)}
            <tr>
              <td>{a.account_id}</td>
              <td>{a.name}</td>
              <td>
                <input
                  class="input input-xs input-bordered w-20 {missing[a.account_id]
                    ? 'input-error'
                    : ''}"
                  bind:value={mapping[a.account_id]}
                />
              </td>
              <td class="text-xs opacity-60">
                {a.reason}{a.standard_name ? " → " + a.standard_name : ""}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <button class="btn btn-sm btn-primary mt-2" onclick={importMapped}>
        Importer med denne mappingen
      </button>
    </div>
  {/if}
</Card>

<Card title="Ingen SAF-T? Legg inn åpningsbalansen manuelt">
  <p class="text-sm opacity-70 mb-2">Saldo per konto på overgangsdagen — må gå i null.</p>
  <div class="flex gap-2 mb-2">
    <input type="date" class="input input-sm input-bordered" bind:value={obDate} />
  </div>
  <div>
    {#each obLines as line, i}
      <div class="flex gap-2 mb-1">
        <input
          class="input input-sm input-bordered w-24"
          placeholder="Konto"
          bind:value={line.account}
        />
        <input
          class="input input-sm input-bordered w-36"
          placeholder={i === 0 ? "Beløp (debet +)" : i === 1 ? "Beløp (kredit −)" : "Beløp"}
          bind:value={line.amount}
        />
      </div>
    {/each}
  </div>
  <div>
    <button
      class="btn btn-xs btn-ghost"
      onclick={() => obLines.push({ account: "", amount: "" })}
    >
      + linje
    </button>
    <button class="btn btn-sm btn-primary" onclick={postOpeningBalance}>
      Legg inn åpningsbalanse
    </button>
  </div>
</Card>
