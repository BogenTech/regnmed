<script>
  // Leverandørene som egen seksjon, speilbildet av Kunder: registeret
  // med søk og sortering, nyregistrering, og partssiden (kontaktinfo,
  // åpne poster, matching) gjenbrukt fra Reskontro. Saldoen står i
  // hovedbokens fortegn — leverandørgjeld er kredit, altså negativ —
  // og «Skyldig» viser den snudd, slik gjelden leses.
  import { api, post } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import Part from "../reskontro/Part.svelte";

  let { companyId, extra } = $props();

  let parties = $state(null);
  let sok = $state("");
  let sortKey = $state("party_no");
  let sortFall = $state(false);

  function reload() {
    api("/companies/" + companyId + "/parties?kind=leverandor")
      .then((svar) => (parties = svar.parties))
      .catch((error) => toast(error.message, false));
  }

  $effect(() => {
    parties = null;
    reload();
  });

  function sorter(key) {
    if (sortKey === key) {
      sortFall = !sortFall;
    } else {
      sortKey = key;
      sortFall = key === "saldo_ore";
    }
  }

  const filtrert = $derived.by(() => {
    if (!parties) return [];
    const q = sok.trim().toLowerCase();
    const treff = q
      ? parties.filter(
          (p) =>
            p.name.toLowerCase().includes(q) ||
            p.party_no.includes(q) ||
            (p.orgnr || "").includes(q) ||
            (p.email || "").toLowerCase().includes(q),
        )
      : parties.slice();
    treff.sort((a, b) => {
      const va = a[sortKey] ?? "";
      const vb = b[sortKey] ?? "";
      const cmp =
        typeof va === "number" ? va - vb : String(va).localeCompare(String(vb), "nb");
      return sortFall ? -cmp : cmp;
    });
    return treff;
  });

  let sumSkyldig = $derived(filtrert.reduce((sum, p) => sum + -p.saldo_ore, 0));

  function pil(key) {
    return sortKey === key ? (sortFall ? " ↓" : " ↑") : "";
  }

  let navn = $state("");
  let orgnr = $state("");

  async function opprett(event) {
    event.preventDefault();
    try {
      const created = await post("/companies/" + companyId + "/parties", {
        kind: "leverandor",
        name: navn,
        orgnr: orgnr || null,
      });
      toast("Leverandør " + created.party_no + " opprettet", true);
      navn = "";
      orgnr = "";
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#if !parties}
  <span class="loading loading-spinner loading-lg"></span>
{:else if extra}
  <Part
    {companyId}
    party={parties.find((p) => p.party_id === extra)}
    partyId={extra}
    tilbake="leverandorer"
  />
{:else}
  <Card title="Ny leverandør">
    <form class="flex flex-wrap gap-2 items-end" onsubmit={opprett}>
      <input class="input" placeholder="Navn" required bind:value={navn} />
      <input class="input w-32" placeholder="Orgnr (valgfritt)" bind:value={orgnr} />
      <button class="btn btn-primary">Opprett</button>
    </form>
  </Card>

  <Card title="Leverandører">
    <input
      class="input input-sm w-full max-w-xs mb-2"
      placeholder="Søk på navn, nummer, orgnr eller e-post"
      bind:value={sok}
    />
    <table class="table table-sm">
      <thead>
        <tr>
          <th class="cursor-pointer" onclick={() => sorter("party_no")}>Nr{pil("party_no")}</th>
          <th class="cursor-pointer" onclick={() => sorter("name")}>Navn{pil("name")}</th>
          <th>Orgnr</th>
          <th>Kontonummer</th>
          <th class="cursor-pointer text-right" onclick={() => sorter("saldo_ore")}>
            Skyldig{pil("saldo_ore")}
          </th>
        </tr>
      </thead>
      <tbody>
        {#each filtrert as p (p.party_id)}
          <tr>
            <td>{p.party_no}</td>
            <td>
              <a class="link" href={"#/c/" + companyId + "/leverandorer/" + p.party_id}>
                {p.name}
              </a>
            </td>
            <td>{p.orgnr || ""}</td>
            <td>{p.bank_account || ""}</td>
            <td class="text-right">{kr(-p.saldo_ore)}</td>
          </tr>
        {:else}
          <tr>
            <td colspan="5" class="opacity-70">
              Ingen leverandører{sok ? " matcher søket" : " ennå"}.
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if filtrert.length}
      <div class="text-right font-semibold">Sum skyldig {kr(sumSkyldig)}</div>
    {/if}
  </Card>
{/if}
