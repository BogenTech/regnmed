<script>
  // Prosjekter og avdelinger som egen seksjon — registeret bodde nederst
  // i Bilag (#37), som er det siste stedet noen som tenker i prosjekter
  // leter. Reglene er uendret: koden er PERMANENT (den inngår i
  // bilagshashen), så en dimensjon avsluttes — slettes aldri; navnet og
  // kundekoblingen (#80) er redigerbar metadata utenfor kjeden.
  //
  // Synlig for alle med DIMENSJON_LES (også ansatte — de fører timer på
  // prosjektene); alt som skriver vises bare med DIMENSJON_SKRIV, og
  // serveren håndhever uansett (docs/auth.md).
  import { api, post, send } from "../../lib/api.js";
  import { kr, minutterTilTimer } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId } = $props();

  let kanSkrive = $derived(harRett(companyId, "DIMENSJON_SKRIV"));

  let data = $state(null);

  async function load(id) {
    const year = new Date().getFullYear();
    const [dims, parties, summary] = await Promise.all([
      api("/companies/" + id + "/dimensions"),
      api("/companies/" + id + "/parties?kind=kunde").catch(() => ({ parties: [] })),
      // Timetallene krever TIMER_RAPPORT_LES — uten den vises registeret
      // uten kolonnene, ikke en feilmelding.
      api(
        "/companies/" + id + "/timesheet/summary?from=" + year + "-01-01&to=" + year + "-12-31",
      ).catch(() => null),
    ]);
    data = {
      dims: dims.dimensions,
      kunder: parties.parties,
      timer: summary ? summary.prosjekter : null,
    };
  }

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    const id = companyId;
    data = null;
    load(id).catch((error) => toast(error.message, false));
  });

  let prosjekter = $derived((data?.dims || []).filter((d) => d.kind === "prosjekt"));
  let avdelinger = $derived((data?.dims || []).filter((d) => d.kind === "avdeling"));

  function timerFor(code) {
    return (data?.timer || []).find((p) => p.prosjekt === code) || null;
  }

  // Ett redigeringsfelt om gangen: {kind, code, navn}.
  let rediger = $state(null);

  async function lagreNavn() {
    try {
      await send(
        "PUT",
        "/companies/" +
          companyId +
          "/dimensions/" +
          rediger.kind +
          "/" +
          encodeURIComponent(rediger.code),
        { name: rediger.navn.trim() },
      );
      rediger = null;
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function toggle(d) {
    try {
      await send(
        "PUT",
        "/companies/" + companyId + "/dimensions/" + d.kind + "/" + encodeURIComponent(d.code),
        { active: !d.active },
      );
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function settKunde(d, partyNo) {
    try {
      await send(
        "PUT",
        "/companies/" + companyId + "/dimensions/" + d.kind + "/" + encodeURIComponent(d.code),
        { kunde: partyNo },
      );
      reload();
    } catch (error) {
      toast(error.message, false);
      reload(); // sett velgeren tilbake til det serveren faktisk mener
    }
  }

  let nyProsjekt = $state({ code: "", name: "", kunde: "" });
  let nyAvdeling = $state({ code: "", name: "" });

  async function opprett(kind, felt) {
    try {
      await post("/companies/" + companyId + "/dimensions", {
        kind,
        code: felt.code.trim(),
        name: felt.name.trim(),
        kunde: kind === "prosjekt" && felt.kunde ? felt.kunde : null,
      });
      toast((kind === "prosjekt" ? "Prosjekt" : "Avdeling") + " opprettet", true);
      felt.code = "";
      felt.name = "";
      if (kind === "prosjekt") felt.kunde = "";
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <Card title="Prosjekter">
    <p class="text-sm opacity-70 mb-2">
      Koden er permanent (den inngår i bilagshashen); navnet kan endres, og et avsluttet
      prosjekt avviser nye posteringer og timer. Knyttes prosjektet til kunden det er for,
      følges timene til kunden og fakturagrunnlaget foreslår mottaker. Lønnsomheten står
      under
      <a class="link" href={"#/c/" + companyId + "/rapporter/prosjekt"}>Rapporter → Prosjekt</a>.
    </p>
    {#if prosjekter.length}
      <table class="table table-sm mb-2">
        <thead>
          <tr>
            <th>Kode</th>
            <th>Navn</th>
            <th>Kunde</th>
            {#if data.timer}
              <th class="text-right">Timer i år</th>
              <th class="text-right">Ufakturert</th>
            {/if}
            <th>Status</th>
            {#if kanSkrive}<th></th>{/if}
          </tr>
        </thead>
        <tbody>
          {#each prosjekter as d (d.code)}
            <tr class={d.active ? "" : "opacity-50"}>
              <td class="font-medium">{d.code}</td>
              <td>
                {#if rediger && rediger.kind === "prosjekt" && rediger.code === d.code}
                  <input class="input input-xs" bind:value={rediger.navn} />
                  <button class="btn btn-xs btn-primary" onclick={lagreNavn}>Lagre</button>
                  <button class="btn btn-xs btn-ghost" onclick={() => (rediger = null)}>
                    Avbryt
                  </button>
                {:else}
                  {d.name}
                {/if}
              </td>
              <td>
                {#if kanSkrive && data.kunder.length}
                  <select
                    class="select select-xs"
                    value={d.kunde || ""}
                    onchange={(e) => settKunde(d, e.currentTarget.value)}
                  >
                    <option value="">—</option>
                    {#each data.kunder as p (p.party_no)}
                      <option value={p.party_no}>{p.party_no} {p.name}</option>
                    {/each}
                  </select>
                {:else if d.kunde}
                  {d.kunde} {d.kunde_navn}
                {/if}
              </td>
              {#if data.timer}
                {@const t = timerFor(d.code)}
                <td class="text-right">{t ? minutterTilTimer(t.minutter) + " t" : ""}</td>
                <td class="text-right">{t && t.ufakturert_ore ? kr(t.ufakturert_ore) : ""}</td>
              {/if}
              <td>
                <span class={"badge badge-xs " + (d.active ? "badge-success" : "badge-ghost")}>
                  {d.active ? "Aktivt" : "Avsluttet"}
                </span>
              </td>
              {#if kanSkrive}
                <td class="text-right whitespace-nowrap">
                  {#if !(rediger && rediger.kind === "prosjekt" && rediger.code === d.code)}
                    <button
                      class="btn btn-xs btn-ghost"
                      onclick={() => (rediger = { kind: "prosjekt", code: d.code, navn: d.name })}
                    >
                      Endre navn
                    </button>
                  {/if}
                  <button class="btn btn-xs btn-ghost" onclick={() => toggle(d)}>
                    {d.active ? "Avslutt" : "Gjenåpne"}
                  </button>
                </td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <p class="text-sm opacity-70 mb-2">Ingen prosjekter ennå.</p>
    {/if}
    {#if kanSkrive}
      <div class="flex gap-2 flex-wrap items-center">
        <input class="input input-sm w-24" placeholder="Kode" bind:value={nyProsjekt.code} />
        <input class="input input-sm" placeholder="Navn" bind:value={nyProsjekt.name} />
        {#if data.kunder.length}
          <select class="select select-sm" bind:value={nyProsjekt.kunde}>
            <option value="">— ingen kunde —</option>
            {#each data.kunder as p (p.party_no)}
              <option value={p.party_no}>{p.party_no} {p.name}</option>
            {/each}
          </select>
        {/if}
        <button class="btn btn-sm btn-primary" onclick={() => opprett("prosjekt", nyProsjekt)}>
          Nytt prosjekt
        </button>
      </div>
    {/if}
  </Card>

  <Card title="Avdelinger">
    <p class="text-sm opacity-70 mb-2">
      Samme regler som prosjekter, uten kundekobling: permanent kode, redigerbart navn,
      avsluttet avviser nye posteringer. Resultatrapporten kan filtreres per avdeling.
    </p>
    {#if avdelinger.length}
      <table class="table table-sm mb-2">
        <thead>
          <tr>
            <th>Kode</th>
            <th>Navn</th>
            <th>Status</th>
            {#if kanSkrive}<th></th>{/if}
          </tr>
        </thead>
        <tbody>
          {#each avdelinger as d (d.code)}
            <tr class={d.active ? "" : "opacity-50"}>
              <td class="font-medium">{d.code}</td>
              <td>
                {#if rediger && rediger.kind === "avdeling" && rediger.code === d.code}
                  <input class="input input-xs" bind:value={rediger.navn} />
                  <button class="btn btn-xs btn-primary" onclick={lagreNavn}>Lagre</button>
                  <button class="btn btn-xs btn-ghost" onclick={() => (rediger = null)}>
                    Avbryt
                  </button>
                {:else}
                  {d.name}
                {/if}
              </td>
              <td>
                <span class={"badge badge-xs " + (d.active ? "badge-success" : "badge-ghost")}>
                  {d.active ? "Aktiv" : "Avsluttet"}
                </span>
              </td>
              {#if kanSkrive}
                <td class="text-right whitespace-nowrap">
                  {#if !(rediger && rediger.kind === "avdeling" && rediger.code === d.code)}
                    <button
                      class="btn btn-xs btn-ghost"
                      onclick={() => (rediger = { kind: "avdeling", code: d.code, navn: d.name })}
                    >
                      Endre navn
                    </button>
                  {/if}
                  <button class="btn btn-xs btn-ghost" onclick={() => toggle(d)}>
                    {d.active ? "Avslutt" : "Gjenåpne"}
                  </button>
                </td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <p class="text-sm opacity-70 mb-2">Ingen avdelinger ennå.</p>
    {/if}
    {#if kanSkrive}
      <div class="flex gap-2 flex-wrap items-center">
        <input class="input input-sm w-24" placeholder="Kode" bind:value={nyAvdeling.code} />
        <input class="input input-sm" placeholder="Navn" bind:value={nyAvdeling.name} />
        <button class="btn btn-sm btn-primary" onclick={() => opprett("avdeling", nyAvdeling)}>
          Ny avdeling
        </button>
      </div>
    {/if}
  </Card>
{/if}
