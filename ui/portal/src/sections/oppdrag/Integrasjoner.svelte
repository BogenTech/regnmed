<script>
  // Maskin-tilgang: en integrasjon er en person av typen «integrasjon»,
  // så tilgangsoppslag og revisjonsspor er de samme som for mennesker.
  // Tilbakekalling virker straks (valid_to er eksklusiv).
  import { post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import Agentansikt from "../../components/Agentansikt.svelte";

  let { companyId, integrasjoner, kall, roller, farger = [], onDone } = $props();

  // Icons an agent can wear: the sections whose work it would do. The
  // set is the portal's own (ikoner.js) — no icon library, same rule as
  // everywhere else. The server only caps the length; an unknown name
  // falls back to initials.
  const IKONVALG = [
    ["", "Initialer"],
    ["bilag", "Bilag"],
    ["hovedbok", "Hovedbok"],
    ["rapporter", "Rapporter"],
    ["faktura", "Faktura"],
    ["bank", "Bank"],
    ["mva", "MVA"],
    ["lonn", "Lønn"],
    ["timer", "Timer"],
    ["oppdrag", "Oppdrag"],
  ];

  let clientId = $state("");
  let navn = $state("");
  let access = $state("les");

  // #101: en agent er en bruker, så den får en ROLLE — også admin, hvis
  // eieren vil det. `revisor` står ikke her av samme grunn som den ikke
  // står i medlemsvelgeren: den følger et revisjonsoppdrag, den tildeles
  // ikke. Grensen som består er serverens: en maskin får aldri en
  // rettighet som styrer hvem som har tilgang.
  // Uten MEDLEM_ADMIN svarer /roles 403, og da står de innebygde navnene
  // her: serveren avviser likevel et navn som ikke er en rolle, så
  // reservelisten kan ikke gi mer tilgang enn den skal.
  const RESERVE = ["les", "bokforing", "ansatt", "admin"];
  const valg = $derived(
    roller
      ? [
          ...roller.innebygde
            .filter((r) => r.navn !== "revisor")
            .map((r) => ({ navn: r.navn, beskrivelse: r.beskrivelse })),
          ...roller.egne
            .filter((r) => r.aktiv)
            .map((r) => ({ navn: r.navn, beskrivelse: "Egendefinert rolle" })),
        ]
      : RESERVE.map((navn) => ({ navn, beskrivelse: "" })),
  );
  const valgtBeskrivelse = $derived(valg.find((r) => r.navn === access)?.beskrivelse || "");

  async function giTilgang() {
    try {
      await post("/companies/" + companyId + "/integrations", {
        client_id: clientId.trim(),
        navn: navn.trim(),
        access,
      });
      toast("Integrasjonen har fått tilgang", true);
      clientId = "";
      navn = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function endre(i) {
    const svar = await skjema("Endre " + i.navn, [
      { navn: "navn", etikett: "Navn", standard: i.navn },
      { navn: "kontakt", etikett: "Kontakt", standard: i.kontakt || "", valgfri: true },
      {
        navn: "avatar_farge",
        etikett: "Farge",
        type: "select",
        standard: i.avatar_farge || "",
        valg: [["", "Standard"], ...farger.map((f) => [f, f])],
      },
      {
        navn: "avatar_ikon",
        etikett: "Ansikt",
        type: "select",
        standard: i.avatar_ikon || "",
        valg: IKONVALG,
      },
    ]);
    if (!svar) return;
    try {
      await send("PUT", "/companies/" + companyId + "/integrations/" + i.integration_id, svar);
      toast("Endringen er lagret", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function trekkTilbake(i) {
    if (
      !(await bekreft("Trekke tilbake tilgangen? Den slutter å virke med én gang.", {
        tittel: "Trekk tilbake",
        ok: "Trekk tilbake",
        farlig: true,
      }))
    ) {
      return;
    }
    try {
      await post("/companies/" + companyId + "/integrations/" + i.integration_id + "/revoke", {});
      toast("Tilgangen er trukket tilbake", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Integrasjoner (maskin-tilgang)">
  <p class="text-sm opacity-70 mb-2">
    Et kassasystem, en nettbutikk eller en agent kaller API-et med sin egen identitet fra
    påloggingstjenesten. Du gir den en rolle, akkurat som du gir et menneske en, og kan trekke
    tilgangen tilbake når som helst — den slutter å virke i samme øyeblikk. Alt den bokfører bærer
    navnet dens.
  </p>
  <p class="text-sm opacity-70 mb-2">
    Én grense gjelder uansett rolle: en maskin kan aldri endre hvem som har tilgang — verken
    medlemmer, oppdrag, integrasjoner eller firmaopplysninger. Den gjør jobben, men utvider ikke
    tilgang.
  </p>
  {#if integrasjoner.length}
    <table class="table table-sm mb-3">
      <thead>
        <tr>
          <th>Integrasjon</th><th>Rolle</th><th>Status</th>
          <th class="text-right">Kall i dag</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each integrasjoner as i (i.integration_id)}
          <tr class={i.aktiv ? "" : "opacity-50"}>
            <td>
              <div class="flex items-center gap-2">
                <Agentansikt
                  navn={i.navn}
                  farge={i.avatar_farge}
                  ikon={i.avatar_ikon}
                  storrelse="w-8"
                />
                <span>
                  {i.navn}<br /><span class="text-xs opacity-60 font-mono">{i.client_id}</span>
                </span>
              </div>
            </td>
            <td>{i.access}</td>
            <td>
              {#if i.aktiv}
                <span class="badge badge-success badge-sm">aktiv</span>
              {:else}
                <span class="badge badge-ghost badge-sm">trukket {i.valid_to || ""}</span>
              {/if}
            </td>
            <td class="text-right">{i.kall_i_dag} / {i.rate_limit_min}/min</td>
            <td>
              {#if i.aktiv}
                <button class="btn btn-xs btn-ghost" onclick={() => endre(i)}>Endre</button>
                <button class="btn btn-xs btn-ghost" onclick={() => trekkTilbake(i)}>
                  Trekk tilbake
                </button>
              {:else}
                {i.revoked_by || ""}
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen integrasjoner ennå.</p>
  {/if}
  <div class="flex gap-2 items-center flex-wrap">
    <input
      class="input input-sm w-56"
      placeholder="client_id fra regnid"
      bind:value={clientId}
    />
    <input class="input input-sm w-40" placeholder="Navn" bind:value={navn} />
    <select class="select select-sm" bind:value={access}>
      {#each valg as r (r.navn)}
        <option value={r.navn}>{r.navn}</option>
      {/each}
    </select>
    <button class="btn btn-sm" onclick={giTilgang}>Gi tilgang</button>
  </div>
  {#if valgtBeskrivelse}
    <p class="text-xs opacity-60 mt-1">{valgtBeskrivelse}</p>
  {/if}
  {#if kall.length}
    <h3 class="font-semibold text-sm mt-3 mb-1">Siste endringer</h3>
    {#each kall.slice(0, 10) as k, i (i)}
      <div class="text-xs opacity-70 py-0.5">
        {k.tidspunkt.slice(0, 16).replace("T", " ")} · {k.navn} · {k.method} {k.path}
      </div>
    {/each}
  {/if}
</Card>
