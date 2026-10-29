<script>
  // Maskin-tilgang: en integrasjon er en person av typen «integrasjon»,
  // så tilgangsoppslag og revisjonsspor er de samme som for mennesker.
  // Tilbakekalling virker straks (valid_to er eksklusiv).
  import { post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import Agentansikt from "../../components/Agentansikt.svelte";

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
    navn: "systemprompt",
    etikett: "Særskilte instrukser",
    type: "textarea",
    standard: i.systemprompt || "",
    valgfri: true,
    hjelp: INSTRUKSHJELP,
  });
  const rollefelt = (standard) => ({
    navn: "access",
    etikett: "Rolle",
    type: "select",
    standard,
    valg: valg.map((r) => [r.navn, r.navn]),
  });

  const INSTRUKSHJELP =
    "Legges til rollens egne regler — kontoene dere bruker, hvem agenten skal spørre. " +
    "Rollen bestemmer fortsatt hva agenten FÅR gjøre.";
  let clientId = $state("");
  let navn = $state("");
  let access = $state("les");

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
        {
          navn: "klient_hemmelighet",
          etikett: "Klienthemmelighet fra regnid",
          type: "password",
          standard: "",
          valgfri: true,
          hjelp:
            "Trengs for at agenten skal kunne arbeide selv — den logger inn som seg selv, " +
            "med samme klient som en agent på din egen maskin. Tom lar den som er lagret stå.",
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

<Card title="Agenter og integrasjoner">
  <p class="text-sm opacity-70 mb-2">
    En agent er en bruker som ikke er et menneske. Den får et navn, et ansikt og en rolle,
    akkurat som en ansatt eller en regnskapsfører, og du kan trekke tilgangen tilbake når som
    helst — den slutter å virke i samme øyeblikk. Alt den bokfører bærer navnet dens. Det samme
    kortet styrer et kassasystem eller en nettbutikk som kaller API-et.
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
          <th>Integrasjon</th><th>Rolle</th><th>Modell</th><th>Status</th>
          <th class="text-right">Kall i dag</th><th></th>
        </tr>
      </thead>
      <tbody>
        <!-- Nøkkelen er GRANTET, ikke agenten: etter et rollebytte står
             samme agent med én avsluttet og én aktiv rad, og to like
             nøkler fryser komponentens reaktivitet (CLAUDE.md punkt 68). -->
        {#each integrasjoner as i (i.grant_id)}
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
            <td>
              {i.access}
              {#if i.systemprompt}
                <br /><span class="text-xs opacity-60" title={i.systemprompt}>
                  + egne instrukser
                </span>
              {/if}
            </td>
            <td>
              {#if i.llm}
                <span class="text-xs">{i.llm.leverandor}<br />{i.llm.modell}</span>
              {:else}
                <span class="text-xs opacity-50">egen agent</span>
              {/if}
            </td>
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
                <button class="btn btn-xs btn-ghost" onclick={() => byttRolle(i)}>
                  Bytt rolle
                </button>
                <button class="btn btn-xs btn-ghost" onclick={() => endre(i)}>Endre</button>
                {#if llmTilgjengelig}
                  <button class="btn btn-xs btn-ghost" onclick={() => settModell(i)}>
                    {i.llm ? "Bytt modell" : "Gi modell"}
                  </button>
                  {#if i.llm}
                    <button class="btn btn-xs btn-ghost" onclick={() => fjernModell(i)}>
                      Slå av
                    </button>
                  {/if}
                {/if}
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
  {#if kanOpprette}
    <button class="btn btn-sm btn-primary" onclick={nyAgent}>Ny agent</button>
    <p class="text-xs opacity-60 mt-1">
      Agenten får sin egen identitet automatisk. Har du alt en agent som kjører på din egen
      maskin, kan du
      <button class="link" onclick={() => (viserEgen = !viserEgen)}>
        gi den tilgang i stedet
      </button>.
    </p>
  {/if}
  {#if !kanOpprette || viserEgen}
    <div class="flex gap-2 items-center flex-wrap {kanOpprette ? 'mt-2' : ''}">
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
