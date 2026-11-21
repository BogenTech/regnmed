<script>
  // Administrasjon — the company's own console. Nothing new is decided
  // here: every card already existed (Firmaopplysninger, Abonnement,
  // Brukere/roller, Oppdrag/integrasjoner, Periode) and every call goes
  // to the same guarded endpoints. The console only gathers them behind
  // one menu entry, with a dashboard that shows the state of each area
  // and what is left to do. The portal SHOWS, the server decides
  // (docs/auth.md).
  //
  // Each tab loads only what it shows: the dashboard's requests run on
  // the dashboard, not before every tab.
  import { api } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import Ikon from "../../components/Ikon.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import Firmaopplysninger from "../oversikt/Firmaopplysninger.svelte";
  import EhfSender from "../oversikt/EhfSender.svelte";
  import Abonnement from "../oversikt/Abonnement.svelte";
  import Brukere from "../brukere/Brukere.svelte";
  import Oppdrag from "../oppdrag/Oppdrag.svelte";
  import Periode from "../periode/Periode.svelte";
  import Kundevilkar from "./Kundevilkar.svelte";
  import { gjoremal } from "./admin.js";

  let { companyId, extra } = $props();

  // Sub-tabs live in the address (#/c/{id}/admin/{fane}) so each one is
  // shareable, like mva-terminene.
  const FANER = [
    ["", "Oversikt", "oversikt"],
    ["firma", "Firmaopplysninger", "selskaper"],
    ["abonnement", "Abonnement", "abonnementer"],
    ["brukere", "Brukere og roller", "brukere"],
    ["oppdrag", "Oppdrag og integrasjoner", "oppdrag"],
    ["periode", "Periode", "periode"],
  ];

  let fane = $derived(extra || "");

  // { ikke: true } = no right to read it (decided from /me, so a refusal
  // never looks like an outage); { feil } = the request failed; anything
  // else is the response.
  let data = $state(null);
  let settings = $state(null); // firma tab
  let abo = $state(null); // abonnement tab

  async function hent(id, sti, rett) {
    if (!harRett(id, rett)) return { ikke: true };
    try {
      return await api("/companies/" + id + sti);
    } catch (error) {
      return { feil: error.message };
    }
  }

  async function lastOversikt() {
    const id = companyId;
    data = null;
    const [settings, abo, terms, access, invitations, integrations, engagements, lock] =
      await Promise.all([
        hent(id, "/settings", "SELSKAP_LES"),
        hent(id, "/subscription", "SELSKAP_LES"),
        hent(id, "/terms", "SELSKAP_LES"),
        hent(id, "/access", "MEDLEM_ADMIN"),
        hent(id, "/invitations", "MEDLEM_ADMIN"),
        hent(id, "/integrations", "INTEGRASJON_LES"),
        hent(id, "/engagements", "OPPDRAG_LES"),
        hent(id, "/period-lock", "BILAG_LES"),
      ]);
    if (id === companyId)
      data = { settings, abo, terms, access, invitations, integrations, engagements, lock };
  }

  async function lastSettings() {
    const id = companyId;
    const svar = await hent(id, "/settings", "SELSKAP_LES");
    if (id === companyId) settings = svar;
  }

  async function lastAbo() {
    const id = companyId;
    const svar = await hent(id, "/subscription", "SELSKAP_LES");
    if (id === companyId) abo = svar;
  }

  $effect(() => {
    void companyId;
    if (fane === "") lastOversikt();
    else if (fane === "firma") {
      settings = null;
      lastSettings();
    } else if (fane === "abonnement") {
      abo = null;
      lastAbo();
    }
  });

  const brukbar = (v) => v && !v.ikke && !v.feil;

  const ABO_TEKST = {
    aktiv: "Aktivt",
    prove: "Prøvetid",
    frist: "Ubetalt",
    sperret: "Sperret",
  };

  let oppgaver = $derived(
    data
      ? gjoremal({
          settings: brukbar(data.settings) ? data.settings : null,
          terms: brukbar(data.terms) ? data.terms : null,
          abo: brukbar(data.abo) ? data.abo : null,
        })
      : [],
  );
  let feilet = $derived(data ? Object.values(data).some((v) => v?.feil) : false);

  // Dashboard tiles: [fane, tittel, kilde, verdi(kilde), beskrivelse(kilde)].
  let fliser = $derived(
    data
      ? [
          [
            "abonnement",
            "Abonnement",
            data.abo,
            (a) => ABO_TEKST[a.status] || a.status,
            (a) => (a.dato ? "til " + formatDate(a.dato) : ""),
          ],
          [
            "brukere",
            "Brukere",
            data.access,
            (a) => a.medlemmer.filter((m) => m.aktiv).length,
            () =>
              brukbar(data.invitations) && data.invitations.invitasjoner.length
                ? data.invitations.invitasjoner.length + " åpne invitasjoner"
                : "",
          ],
          [
            "oppdrag",
            "Oppdrag",
            data.engagements,
            (e) => e.engagements.filter((x) => !x.valid_to).length,
            () => {
              const n = brukbar(data.integrations)
                ? data.integrations.integrasjoner.filter((i) => i.aktiv).length
                : 0;
              return n ? n + " integrasjoner" : "";
            },
          ],
          [
            "periode",
            "Periode låst t.o.m.",
            data.lock,
            (l) => (l.locked_through ? formatDate(l.locked_through) : "åpen"),
            () => "",
          ],
          [
            "firma",
            "Firmaopplysninger",
            data.settings,
            () => {
              // The terms live on this tab too, but are not company data.
              const n = oppgaver.filter((o) => o.fane === "firma" && !o.vilkar).length;
              return n ? n + " mangler" : "utfylt";
            },
            (s) => s.orgnr || "",
          ],
        ]
      : [],
  );
</script>

<nav aria-label="Administrasjon" class="tabs tabs-box tabs-sm mb-4 flex-wrap">
  {#each FANER as [slug, navn, ikonNavn] (slug)}
    <a
      href={"#/c/" + companyId + "/admin" + (slug ? "/" + slug : "")}
      class="tab gap-1.5 {fane === slug ? 'tab-active' : ''}"
      aria-current={fane === slug ? "page" : undefined}
    >
      <Ikon navn={ikonNavn} />
      {navn}
    </a>
  {/each}
</nav>

{#if fane === ""}
  {#if !data}
    <span class="loading loading-spinner loading-md" aria-label="Laster oversikten"></span>
  {:else}
    {#if feilet}
      <div class="mb-4">
        <LoadError error="Noe av oversikten kunne ikke hentes." retry={lastOversikt} />
      </div>
    {/if}
    {#if oppgaver.length}
      <section class="card card-border border-base-300 bg-base-100 mb-4" aria-labelledby="admin-gjore">
        <div class="card-body p-4">
          <h2 id="admin-gjore" class="font-semibold">Å gjøre</h2>
          <ul class="list-disc ml-5 text-sm space-y-1">
            {#each oppgaver as o (o.tekst)}
              <li><a class="link" href={"#/c/" + companyId + "/admin/" + o.fane}>{o.tekst}</a></li>
            {/each}
          </ul>
        </div>
      </section>
    {/if}
    <!-- daisyUI stat tiles, same shape as the platform dashboard's. -->
    <div class="stats border border-base-300 bg-base-100 w-full stats-vertical lg:stats-horizontal">
      {#each fliser as [slug, tittel, kilde, verdi, beskrivelse] (slug)}
        <a href={"#/c/" + companyId + "/admin/" + slug} class="stat">
          <div class="stat-figure opacity-60">
            <Ikon navn={FANER.find(([f]) => f === slug)?.[2] || slug} />
          </div>
          <div class="stat-title">{tittel}</div>
          {#if kilde?.ikke}
            <div class="stat-value text-sm opacity-50 whitespace-normal">krever mer tilgang</div>
          {:else if kilde?.feil}
            <div class="stat-value text-sm text-error whitespace-normal">kunne ikke hentes</div>
          {:else}
            <div class="stat-value text-2xl">{verdi(kilde)}</div>
            {#if beskrivelse(kilde)}<div class="stat-desc">{beskrivelse(kilde)}</div>{/if}
          {/if}
        </a>
      {/each}
    </div>
  {/if}
{:else if fane === "firma"}
  {#if !settings}
    <span class="loading loading-spinner loading-md" aria-label="Laster firmaopplysningene"></span>
  {:else if settings.feil}
    <LoadError error={settings.feil} retry={lastSettings} />
  {:else if settings.ikke}
    <div class="alert alert-info"><span>Firmaopplysninger krever lesetilgang til selskapet.</span></div>
  {:else}
    <Firmaopplysninger {companyId} {settings} onDone={lastSettings} />
    <EhfSender {companyId} />
    <Kundevilkar {companyId} />
  {/if}
{:else if fane === "abonnement"}
  {#if !abo}
    <span class="loading loading-spinner loading-md" aria-label="Laster abonnementet"></span>
  {:else if abo.feil}
    <LoadError error={abo.feil} retry={lastAbo} />
  {:else if abo.ikke}
    <div class="alert alert-info"><span>Abonnementet administreres av selskapets admin.</span></div>
  {:else}
    <Abonnement {companyId} {abo} onDone={lastAbo} />
  {/if}
{:else if fane === "brukere"}
  <Brukere {companyId} />
{:else if fane === "oppdrag"}
  <Oppdrag {companyId} />
{:else if fane === "periode"}
  <Periode {companyId} />
{:else}
  <div class="alert alert-warning"><span>Ukjent fane «{fane}».</span></div>
{/if}
