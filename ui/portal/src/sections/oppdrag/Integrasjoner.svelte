<script>
  // Maskin-tilgang: en integrasjon er en person av typen «integrasjon»,
  // så tilgangsoppslag og revisjonsspor er de samme som for mennesker.
  // Tilbakekalling virker straks (valid_to er eksklusiv).
  import { api, post, send } from "../../lib/api.js";
  import { naadd } from "../../lib/pakke.js";
  import { formatDate, formatTime } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import Agentansikt from "../../components/Agentansikt.svelte";
  import { rolleNavn } from "../brukere/brukere.js";

  let {
    companyId,
    integrasjoner,
    kall,
    roller,
    farger = [],
    llmTilgjengelig = false,
    leverandorer = [],
    kanOpprette = false,
    onDone,
  } = $props();

  // The package limit shown up front; the server refuses regardless.
  let pakke = $state(null);
  $effect(() => {
    const id = companyId;
    void integrasjoner?.length; // read again after the list changes: the count is what fills up
    pakke = null;
    api("/companies/" + id + "/subscription")
      .then((s) => {
        if (companyId === id) pakke = s;
      })
      .catch(() => {});
  });
  let fullt = $derived(!!pakke?.bruk && naadd(pakke.bruk.integrasjoner, pakke.bruk.integrasjoner_grense));

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

  // Den gamle veien inn — en agent kunden kjører selv og alt har en
  // klient-id for — er fortsatt her, men skjult når serveren kan lage
  // identiteten selv: den er unntaket nå, ikke normalen.
  let viserEgen = $state(false);
  // Feltene begge dialogene spør om, ett sted: nytt ansikt og endret
  // ansikt er samme spørsmål, og to kopier ville svart ulikt før eller
  // senere.
  const ansiktsfelt = (i = {}) => [
    {
      navn: "avatar_farge",
      etikett: "Farge",
      type: "select",
      standard: i.avatar_farge || "",
      valg: [["", "Standard"], ...farger.map((f) => [f, f])],
    },
    { navn: "avatar_ikon", etikett: "Ansikt", type: "select", standard: i.avatar_ikon || "", valg: IKONVALG },
  ];
  // Instruksene hører sammen med rollen, ikke med ansiktet: de to
  // avgjør hva agenten gjør, resten er utseende.
  const instruksfelt = (i = {}) => ({
    navn: "system_prompt",
    etikett: "Særskilte instrukser",
    type: "textarea",
    standard: i.system_prompt || "",
    valgfri: true,
    hjelp: INSTRUKSHJELP,
  });
  const rollefelt = (standard) => ({
    navn: "access",
    etikett: "Rolle",
    type: "select",
    standard,
    valg: valg.map((r) => [r.navn, rolleNavn(r.navn)]),
  });

  const INSTRUKSHJELP =
    "Legges til rollens egne regler — kontoene dere bruker, hvem agenten skal spørre. " +
    "Rollen bestemmer fortsatt hva agenten FÅR gjøre.";
  let clientId = $state("");
  let navn = $state("");
  let access = $state("les");
  let opptatt = $state(false);

  // Ny agent i ETT skjema (docs/agent.md §8.6). Identiteten lager
  // serveren — brukeren skal ikke trenge å vite at agenten har en
  // klient-id i det hele tatt, like lite som en ansatt trenger å vite
  // hvordan innloggingen virker.

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

  async function giTilgang(event) {
    event.preventDefault();
    if (opptatt) return;
    opptatt = true;
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
    } finally {
      opptatt = false;
    }
  }

  async function nyAgent() {
    const svar = await skjema(
      "Ny agent",
      [
        { navn: "navn", etikett: "Navn", standard: "" },
        rollefelt("bokforing"),
        instruksfelt(),
        {
          navn: "leverandor",
          etikett: "Leverandør",
          type: "select",
          standard: "openrouter",
          valg: leverandorer.map((l) => [l.navn, l.navn]),
        },
        {
          navn: "base_url",
          etikett: "Adresse (kreves for «annet»)",
          standard: "",
          valgfri: true,
          hjelp: "Må være https. Tom bruker leverandørens egen adresse.",
        },
        { navn: "modell", etikett: "Modell", standard: "", valgfri: true },
        {
          navn: "api_nokkel",
          etikett: "API-nøkkel",
          type: "password",
          standard: "",
          valgfri: true,
          hjelp: "Lagres kryptert, vises aldri igjen. Tom: modell senere.",
        },
        ...ansiktsfelt(),
      ],
      {
        melding:
          "Agenten blir en bruker med den rollen du gir den, og rollen er grensen: " +
          "den kan ikke gjøre noe utenfor den, og aldri endre hvem som har tilgang. " +
          "Med modell og nøkkel sender den regnskapsdata til leverandøren du velger, " +
          "på din egen regning.",
        ok: "Opprett",
      },
    );
    if (!svar) return;
    try {
      const resultat = await post("/companies/" + companyId + "/agents", svar);
      for (const advarsel of resultat.advarsler || []) toast(advarsel, false);
      toast(resultat.klar ? "Agenten er klar til å arbeide" : "Agenten er opprettet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Rollen er sikkerhetsgrensen, så den skal kunne endres der den
  // vises. Under panseret er det fortsatt tilbakekalling + nytt grant
  // (et grant er uforanderlig), så det daterte sporet består.
  async function byttRolle(i) {
    const svar = await skjema("Rolle for " + i.navn, [rollefelt(i.access)]);
    if (!svar || svar.access === i.access) return;
    try {
      await send(
        "PUT",
        "/companies/" + companyId + "/integrations/" + i.integration_id + "/rolle",
        svar,
      );
      toast("Rollen er endret", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function endre(i) {
    const svar = await skjema("Endre " + i.navn, [
      { navn: "navn", etikett: "Navn", standard: i.navn },
      { navn: "kontakt", etikett: "Kontakt", standard: i.kontakt || "", valgfri: true },
      instruksfelt(i),
      ...ansiktsfelt(i),
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

  // Vertsagent (docs/agent.md §8). Leverandøren blir underdatabehandler
  // for selskapets regnskapsdata, så valget tas her, synlig, og ikke i
  // en avtaletekst — derfor står det i dialogen og ikke bare i doksene.
  async function settModell(i) {
    const svar = await skjema(
      "Modell for " + i.navn,
      [
        {
          navn: "leverandor",
          etikett: "Leverandør",
          type: "select",
          standard: i.llm?.leverandor || "openai",
          valg: leverandorer.map((l) => [l.navn, l.navn]),
        },
        {
          navn: "base_url",
          etikett: "Adresse (kreves for «annet»)",
          standard: "",
          valgfri: true,
          hjelp: "Må være https. Tom bruker leverandørens egen adresse.",
        },
        { navn: "modell", etikett: "Modell", standard: i.llm?.modell || "" },
        {
          navn: "api_nokkel",
          etikett: "API-nøkkel",
          type: "password",
          standard: "",
          hjelp: "Lagres kryptert og vises aldri igjen.",
        },
      ],
      {
        melding:
          "Agenten sender regnskapsdata til leverandøren du velger, med din egen nøkkel og " +
          "på din egen regning. Leverandøren blir databehandler for de dataene. " +
          "Nøkkelen lagres kryptert og kan ikke leses ut igjen — bare erstattes.",
        ok: "Lagre",
      },
    );
    if (!svar) return;
    try {
      await send("PUT", "/companies/" + companyId + "/integrations/" + i.integration_id + "/llm", svar);
      toast("Modellen er lagret", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function fjernModell(i) {
    if (
      !(await bekreft("Slå av modellen for " + i.navn + "? Agenten slutter å arbeide selv.", {
        tittel: "Slå av modellen",
        ok: "Slå av",
        farlig: true,
      }))
    ) {
      return;
    }
    try {
      await send(
        "DELETE",
        "/companies/" + companyId + "/integrations/" + i.integration_id + "/llm",
      );
      toast("Modellen er slått av", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function trekkTilbake(i) {
    if (
      !(await bekreft("Trekke tilbake tilgangen for " + i.navn + "? Den slutter å virke med én gang.", {
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

<Card title="Agenter og integrasjoner">
  <p class="text-sm opacity-70 mb-2">
    En agent er en bruker som ikke er et menneske. Den får et navn, et ansikt og en rolle, akkurat
    som en ansatt, og alt den bokfører bærer navnet dens. Du kan trekke tilgangen tilbake når som
    helst, og da slutter den å virke med én gang. Det samme gjelder et kassasystem eller en
    nettbutikk som bruker API-et.
  </p>
  <p class="text-sm opacity-70 mb-3">
    Uansett rolle kan en maskin aldri endre hvem som har tilgang: verken medlemmer, oppdrag,
    integrasjoner eller firmaopplysninger.
  </p>
  {#if integrasjoner.length}
    <!-- Nøkkelen er GRANTET, ikke agenten: etter et rollebytte står
         samme agent med én avsluttet og én aktiv rad, og to like
         nøkler fryser komponentens reaktivitet (CLAUDE.md punkt 68).
         Rows, not a table, so every action is reachable on a phone. -->
    <ul class="divide-y divide-base-200 mb-3" aria-label="Agenter og integrasjoner">
      {#each integrasjoner as i (i.grant_id)}
        <li class="py-2 flex flex-wrap items-center gap-x-3 gap-y-2 {i.aktiv ? '' : 'opacity-60'}">
          <Agentansikt navn={i.navn} farge={i.avatar_farge} ikon={i.avatar_ikon} storrelse="w-8" />
          <div class="min-w-0 flex-1 basis-48">
            <div class="font-medium">
              {i.navn}
              {#if i.aktiv}
                <span class="badge badge-success badge-sm ml-1">aktiv</span>
              {:else}
                <span class="badge badge-ghost badge-sm ml-1">
                  trukket tilbake{i.valid_to ? " " + formatDate(i.valid_to) : ""}
                </span>
              {/if}
            </div>
            <div class="text-xs opacity-70">
              {rolleNavn(i.access)}{i.system_prompt ? ", med egne instrukser" : ""}.
              {#if i.llm}Modell: {i.llm.leverandor} {i.llm.modell}.{#if i.aktiv && !i.klient_lagret}{" "}<span class="text-warning">Kan ikke kjøre ennå: velg «Bytt modell» og lagre, så ordner regnmed resten.</span
                  >{/if}{:else}Kjører hos dere selv.{/if}
              {#if i.aktiv}{i.kall_i_dag} kall i dag, høyst {i.rate_limit_min} i minuttet.{/if}
              {#if !i.aktiv && i.revoked_by}Trukket av {i.revoked_by}.{/if}
            </div>
            <div class="text-xs opacity-50 font-mono break-all">{i.client_id}</div>
          </div>
          {#if i.aktiv}
            <div class="flex gap-2 items-center ml-auto">
              <button class="btn btn-sm btn-outline" onclick={() => endre(i)}>Endre</button>
              <details class="dropdown dropdown-end">
                <summary class="btn btn-sm btn-ghost" aria-label={"Flere valg for " + i.navn}>Mer</summary>
                <ul
                  class="dropdown-content menu menu-sm bg-base-200 rounded-box z-10 mt-1 w-48 p-2 shadow"
                  onclick={(e) => (e.currentTarget.closest("details").open = false)}
                  role="presentation"
                >
                  <li><button onclick={() => byttRolle(i)}>Bytt rolle</button></li>
                  {#if llmTilgjengelig}
                    <li>
                      <button onclick={() => settModell(i)}>{i.llm ? "Bytt modell" : "Gi modell"}</button>
                    </li>
                    {#if i.llm}<li><button onclick={() => fjernModell(i)}>Slå av modellen</button></li>{/if}
                  {/if}
                  <li><button class="text-error" onclick={() => trekkTilbake(i)}>Trekk tilbake</button></li>
                </ul>
              </details>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen agenter eller integrasjoner ennå.</p>
  {/if}
  {#if fullt}
    <p class="text-sm mb-2" role="note">
      Pakken {pakke.pakke?.navn ?? ""} har plass til {pakke.bruk.integrasjoner_grense}
      {pakke.bruk.integrasjoner_grense === 1 ? "integrasjon eller agent" : "integrasjoner og agenter"}. Bytt til
      en større pakke under
      <a class="link" href={"#/c/" + companyId + "/admin/abonnement"}>Abonnement</a> for å legge til flere.
    </p>
  {/if}
  {#if kanOpprette}
    <button class="btn btn-sm btn-primary" onclick={nyAgent} disabled={fullt}>Ny agent</button>
    <p class="text-xs opacity-60 mt-1">
      Agenten får sin egen identitet automatisk. Har du alt en agent som kjører på din egen
      maskin, kan du
      <button class="link" aria-expanded={viserEgen} onclick={() => (viserEgen = !viserEgen)}>
        gi den tilgang i stedet
      </button>.
    </p>
  {/if}
  {#if !kanOpprette || viserEgen}
    <form class="flex gap-2 items-end flex-wrap {kanOpprette ? 'mt-2' : ''}" onsubmit={giTilgang}>
      <label class="fieldset py-0 grow basis-48">
        <span class="fieldset-legend">Klient-id fra regnid</span>
        <input class="input input-sm w-full font-mono" required autocomplete="off" bind:value={clientId} />
      </label>
      <label class="fieldset py-0 grow basis-40">
        <span class="fieldset-legend">Navn</span>
        <input class="input input-sm w-full" required bind:value={navn} />
      </label>
      <label class="fieldset py-0">
        <span class="fieldset-legend">Rolle</span>
        <select class="select select-sm w-40" bind:value={access}>
          {#each valg as r (r.navn)}
            <option value={r.navn}>{rolleNavn(r.navn)}</option>
          {/each}
        </select>
      </label>
      <button class="btn btn-sm" disabled={opptatt || fullt}>
        {#if opptatt}<span class="loading loading-spinner loading-xs"></span>{/if}
        Gi tilgang
      </button>
    </form>
    {#if valgtBeskrivelse}
      <p class="text-xs opacity-60 mt-1">{valgtBeskrivelse}</p>
    {/if}
  {/if}
  {#if kall.length}
    <h3 class="font-semibold text-sm mt-4 mb-1">Siste endringer</h3>
    <ul class="text-xs opacity-70" aria-label="Siste endringer fra agenter og integrasjoner">
      {#each kall.slice(0, 10) as k, n (n)}
        <li class="py-0.5 break-all">
          <span class="tabular-nums">{formatTime(k.tidspunkt)}</span>, {k.navn}:
          <span class="font-mono">{k.method} {k.path}</span>
        </li>
      {/each}
    </ul>
  {/if}
</Card>
