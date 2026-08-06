<script>
  // Bokføring fra innboksen: bilaget, vedlegget og statusen settes i ÉN
  // transaksjon på serveren (docs/bilagsinnboks.md) — skjemaet her fyller
  // bare ut linjene. Et forslag (#34) kan forhåndsutfylle dem; det er
  // ALLTID synlig som forslag, og ingenting bokføres før noen trykker.
  import { untrack } from "svelte";
  import { post } from "../../lib/api.js";
  import { today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import DimSelect from "../../components/DimSelect.svelte";

  let { companyId, documentId, dims, forslag = null, onDone } = $props();

  function tomLinje() {
    return { account: "", amount: "", vat: "", party_no: null, avdeling: "", prosjekt: "" };
  }

  let date = $state(untrack(() => forslag?.date || today()));
  let description = $state(untrack(() => forslag?.description || ""));
  let lines = $state(untrack(() => forslag?.lines || [tomLinje(), tomLinje()]));

  async function bokfor() {
    const payload = lines
      .filter((l) => l.account.trim() && l.amount.trim())
      .map((l) => ({
        account: l.account.trim(),
        // kr() bruker typografisk minus; parseren vil ha ASCII.
        amount_ore: Math.round(
          Number(l.amount.trim().replace(/\s/g, "").replace("−", "-").replace(",", ".")) * 100,
        ),
        vat_code: l.vat.trim() || null,
        party_no: l.party_no || null,
        avdeling: l.avdeling || null,
        prosjekt: l.prosjekt || null,
      }));
    try {
      const posted = await post("/companies/" + companyId + "/inbox/" + documentId + "/bokfor", {
        journal_code: "GL",
        date,
        description: description || "Bokført fra innboks",
        lines: payload,
      });
      toast("Bokført som bilag " + posted.voucher, true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<div class="card border border-base-300 card-sm mt-3">
  <div class="card-body">
    {#if forslag}
      <div class="alert alert-info text-sm py-2 mb-2">
        <div>
          <strong>Forslag</strong> ({forslag.kildeTekst}) — kontroller tallene før du bokfører.
          {#if forslag.hvorfor.length}
            <br />
            <span class="text-xs opacity-80">
              {#each forslag.hvorfor as h, i}{#if i}<br />{/if}{h}{/each}
            </span>
          {/if}
        </div>
      </div>
    {/if}
    <p class="text-sm font-semibold mb-2">Bokfør dokument</p>
    <div class="flex gap-2 mb-2">
      <input type="date" class="input input-sm" bind:value={date} />
      <input class="input input-sm flex-1" placeholder="Tekst" bind:value={description} />
    </div>
    <div>
      {#each lines as line}
        <div class="flex gap-2 mb-1">
          <input
            class="input input-sm w-24"
            placeholder="Konto"
            bind:value={line.account}
          />
          <input
            class="input input-sm w-32"
            placeholder="Beløp (f.eks. -125,50)"
            bind:value={line.amount}
          />
          <input class="input input-sm w-16" placeholder="Mva" bind:value={line.vat} />
          <DimSelect
            {dims}
            kind="avdeling"
            cls="select select-sm w-28"
            bind:value={line.avdeling}
          />
          <DimSelect
            {dims}
            kind="prosjekt"
            cls="select select-sm w-28"
            bind:value={line.prosjekt}
          />
        </div>
      {/each}
    </div>
    <button class="btn btn-xs btn-ghost" onclick={() => lines.push(tomLinje())}>+ linje</button>
    <button class="btn btn-sm btn-primary" onclick={bokfor}>Bokfør</button>
  </div>
</div>
