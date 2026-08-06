<script>
  // Administrasjon — the company's own console. Nothing new is decided
  // here: every card already existed (Firmaopplysninger, Abonnement,
  // Brukere/roller, Oppdrag/integrasjoner, Periode) and every call goes
  // to the same guarded endpoints. The console only gathers them behind
  // one menu entry, with a dashboard that shows the state of each area.
  // The portal SHOWS, the server decides (docs/auth.md) — a tile whose
  // endpoint refused simply reports that instead of a number.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Ikon from "../../components/Ikon.svelte";
  import Firmaopplysninger from "../oversikt/Firmaopplysninger.svelte";
  import Abonnement from "../oversikt/Abonnement.svelte";
  import Brukere from "../brukere/Brukere.svelte";
  import Oppdrag from "../oppdrag/Oppdrag.svelte";
  import Periode from "../periode/Periode.svelte";

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

  let data = $state(null);

  // The dashboard's facts. Every fetch degrades to null on refusal —
  // a bokfører without medlemsadministrasjon still gets a dashboard,
  // just with fewer numbers.
  async function load(id) {
    const [settings, abo, access, invitations, integrations, engagements, lock] =
      await Promise.all([
        api("/companies/" + id + "/settings").catch(() => null),
        api("/companies/" + id + "/subscription").catch(() => null),
        api("/companies/" + id + "/access").catch(() => null),
        api("/companies/" + id + "/invitations").catch(() => null),
        api("/companies/" + id + "/integrations").catch(() => null),
        api("/companies/" + id + "/engagements").catch(() => null),
        api("/companies/" + id + "/period-lock").catch(() => null),
      ]);
    data = { settings, abo, access, invitations, integrations, engagements, lock };
  }

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    data = null;
    load(companyId).catch((error) => toast(error.message, false));
  });

  const ABO_TEKST = {
    aktiv: "Aktivt",
    prove: "Prøvetid",
    frist: "Ubetalt",
    sperret: "Sperret",
  };

  let medlemmer = $derived(
    data?.access ? data.access.medlemmer.filter((m) => m.aktiv).length : null,
  );
  // The invitation list IS the open ones — redeemed invitations leave it.
  let apneInvitasjoner = $derived(data?.invitations ? data.invitations.invitasjoner.length : null);
  let aktiveOppdrag = $derived(
    data?.engagements ? data.engagements.engagements.filter((e) => !e.valid_to).length : null,
  );
  let aktiveIntegrasjoner = $derived(
    data?.integrations ? data.integrations.integrasjoner.filter((i) => i.aktiv).length : null,
  );

  // Dashboard tiles: [fane, tittel, verdi, beskrivelse]. Null value =
  // the endpoint refused; the tile says so instead of pretending zero.
  let fliser = $derived(
    data
      ? [
          [
            "abonnement",
            "Abonnement",
            data.abo ? ABO_TEKST[data.abo.status] || data.abo.status : null,
            data.abo?.dato ? "til " + data.abo.dato : "",
          ],
          [
            "brukere",
            "Brukere",
            medlemmer,
            apneInvitasjoner ? apneInvitasjoner + " åpne invitasjoner" : "",
          ],
          [
            "oppdrag",
            "Oppdrag",
            aktiveOppdrag,
            aktiveIntegrasjoner ? aktiveIntegrasjoner + " integrasjoner" : "",
          ],
          [
            "periode",
            "Periode låst t.o.m.",
            data.lock ? data.lock.locked_through || "åpen" : null,
            "",
          ],
          [
            "firma",
            "Firmaopplysninger",
            data.settings ? (data.settings.address ? "utfylt" : "mangler") : null,
            data.settings?.orgnr || "",
          ],
        ]
      : [],
  );
</script>

<div role="tablist" class="tabs tabs-box tabs-sm mb-4 flex-wrap">
  {#each FANER as [slug, navn, ikonNavn] (slug)}
    <a
      role="tab"
      href={"#/c/" + companyId + "/admin" + (slug ? "/" + slug : "")}
      class="tab gap-1.5 {fane === slug ? 'tab-active' : ''}"
      aria-selected={fane === slug}
    >
      <Ikon navn={ikonNavn} />
      {navn}
    </a>
  {/each}
</div>

{#if fane === ""}
  {#if !data}
    <span class="loading loading-spinner loading-lg"></span>
  {:else}
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3 mb-6">
      {#each fliser as [slug, tittel, verdi, beskrivelse] (slug)}
        <a
          href={"#/c/" + companyId + "/admin/" + slug}
          class="card border border-base-300 bg-base-100 transition-colors hover:border-primary"
        >
          <div class="card-body py-4">
            <div class="flex items-center gap-2 text-sm opacity-70">
              <Ikon navn={FANER.find(([f]) => f === slug)?.[2] || slug} />
              {tittel}
            </div>
            {#if verdi === null}
              <div class="text-sm opacity-50">krever mer tilgang</div>
            {:else}
              <div class="text-2xl font-semibold">{verdi}</div>
            {/if}
            {#if beskrivelse}
              <div class="text-xs opacity-60">{beskrivelse}</div>
            {/if}
          </div>
        </a>
      {/each}
    </div>
    <p class="text-sm opacity-60">
      Alt her finnes også som API-endepunkter (docs/api.md) — konsollen samler bare kortene.
      Tilgangen avgjøres av serveren per handling.
    </p>
  {/if}
{:else if fane === "firma"}
  {#if !data}
    <span class="loading loading-spinner loading-lg"></span>
  {:else if data.settings}
    <Firmaopplysninger {companyId} settings={data.settings} />
  {:else}
    <div class="alert alert-info"><span>Firmaopplysninger krever lesetilgang til selskapet.</span></div>
  {/if}
{:else if fane === "abonnement"}
  {#if !data}
    <span class="loading loading-spinner loading-lg"></span>
  {:else if data.abo}
    <Abonnement {companyId} abo={data.abo} onDone={reload} />
  {:else}
    <div class="alert alert-info"><span>Abonnementet administreres av selskapets admin.</span></div>
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
