<script>
  // The guided registration flow (#77): route the user to the right
  // path first — own company (30-day trial, then subscription), byrå
  // (free, gated on Finanstilsynet autorisasjon), or invited (access
  // arrives by itself: /me redeems invitations at login). The registry
  // preview's autorisasjon flags steer the routing both ways, but never
  // decide: an authorized byrå may still keep its own books as a company.
  import { api, post } from "../lib/api.js";
  import { me, loadMe } from "../lib/me.svelte.js";
  import { toast } from "../lib/toast.svelte.js";

  let { onFirmsChanged = async () => {} } = $props();

  const VALG = [
    {
      id: "selskap",
      tittel: "Egen virksomhet",
      tekst: "Jeg skal føre regnskapet for min egen virksomhet.",
      badges: [{ tekst: "30 dager gratis prøvetid", stil: "badge-success" }],
    },
    {
      id: "byra",
      tittel: "Regnskapsfører eller revisor",
      tekst: "Vi fører regnskap eller reviderer for andre.",
      badges: [
        { tekst: "Gratis for byrået", stil: "badge-success" },
        { tekst: "Krever autorisasjon", stil: "badge-ghost" },
      ],
    },
    {
      id: "invitert",
      tittel: "Jeg er invitert",
      tekst: "Arbeidsgiveren eller regnskapsføreren min bruker regnmed allerede.",
      badges: [],
    },
  ];

  let valg = $state(null);
  let orgnr = $state("");
  let preview = $state(null);
  let kind = $state("regnskap");
  let travelt = $state(false);
  let sjekkResultat = $state("");

  const autorisert = $derived(
    preview && (preview.autorisasjon.regnskap || preview.autorisasjon.revisjon),
  );
  const sperret = $derived(preview && (preview.slettet || preview.konkurs));

  function velg(id) {
    valg = id;
    sjekkResultat = "";
    // Keep an existing preview when switching between the two creating
    // paths — the routing hints ("this looks like a byrå") depend on it.
    if (id === "byra" && preview) {
      kind = preview.autorisasjon.regnskap ? "regnskap" : "revisjon";
    }
  }

  async function slaOpp() {
    preview = null;
    try {
      preview = await api("/registry/enheter/" + encodeURIComponent(orgnr.trim()));
      if (valg === "byra") {
        kind = preview.autorisasjon.regnskap ? "regnskap" : "revisjon";
      }
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function opprettSelskap() {
    travelt = true;
    try {
      const created = await post("/companies", { orgnr: orgnr.trim() });
      toast(created.navn + " opprettet med " + created.seeded_accounts + " kontoer", true);
      await loadMe(true);
      location.hash = "#/c/" + created.company_id + "/oversikt";
    } catch (error) {
      toast(error.message, false);
    } finally {
      travelt = false;
    }
  }

  async function registrerByra() {
    travelt = true;
    try {
      const firm = await post("/firms", { orgnr: orgnr.trim(), kind });
      toast(firm.navn + " registrert som autorisert byrå", true);
      await onFirmsChanged();
      location.hash = "#/byra/" + firm.firm_id;
    } catch (error) {
      toast(error.message, false);
    } finally {
      travelt = false;
    }
  }

  async function sjekkInvitasjoner() {
    travelt = true;
    try {
      await loadMe(true);
      sjekkResultat =
        me.nyeTilganger > 0
          ? "Du har fått " + me.nyeTilganger + " ny(e) tilgang(er)."
          : me.companies.length > 0
            ? "Tilgangen din er på plass — velg selskap over."
            : "Ingen invitasjon funnet ennå. Be avsenderen invitere " +
              (me.email || "adressen du logger inn med") +
              ", og sjekk igjen.";
    } catch (error) {
      toast(error.message, false);
    } finally {
      travelt = false;
    }
  }
</script>

<div class="grid gap-4 sm:grid-cols-3">
  {#each VALG as v (v.id)}
    <button
      class={"card bg-base-100 shadow-sm text-left transition-shadow hover:shadow-md border-2 " +
        (valg === v.id ? "border-primary" : "border-transparent")}
      onclick={() => velg(v.id)}
    >
      <div class="card-body p-4">
        <h3 class="card-title text-base">{v.tittel}</h3>
        <p class="text-sm opacity-70">{v.tekst}</p>
        {#if v.badges.length}
          <div class="flex flex-wrap gap-1 mt-1">
            {#each v.badges as b (b.tekst)}
              <span class={"badge badge-sm " + b.stil}>{b.tekst}</span>
            {/each}
          </div>
        {/if}
      </div>
    </button>
  {/each}
</div>

{#if valg === "invitert"}
  <div class="card card-sm bg-base-200 mt-4">
    <div class="card-body">
      <p class="text-sm">
        Da trenger du ikke registrere noe selv: be den som har selskapet i regnmed om å invitere
        <span class="font-semibold">{me.email || "e-postadressen du logger inn med"}</span>
        (under Oppdrag → Tilgang). Invitasjonen løses inn automatisk neste gang du logger inn — eller
        sjekk nå:
      </p>
      <button class="btn btn-sm mt-3" disabled={travelt} onclick={sjekkInvitasjoner}>
        Sjekk på nytt
      </button>
      {#if sjekkResultat}
        <p class="text-sm mt-2 opacity-80">{sjekkResultat}</p>
      {/if}
    </div>
  </div>
{:else if valg}
  <div class="mt-4">
    <p class="text-sm opacity-70 mb-2">
      {valg === "byra"
        ? "Byråets organisasjonsnummer — navnet hentes fra Enhetsregisteret, og autorisasjonen sjekkes i Finanstilsynets register."
        : "Organisasjonsnummeret til virksomheten — navnet hentes fra Enhetsregisteret."}
    </p>
    <div class="flex gap-2">
      <input
        class="input"
        placeholder="Organisasjonsnummer"
        maxlength="9"
        bind:value={orgnr}
        onkeydown={(e) => e.key === "Enter" && slaOpp()}
      />
      <button class="btn" onclick={slaOpp}>Slå opp</button>
    </div>

    {#if preview}
      <div class="card card-sm bg-base-200 mt-4">
        <div class="card-body">
          <p class="font-semibold">{preview.navn} ({preview.organisasjonsform || ""})</p>
          <p class="text-sm opacity-70">{preview.naeringskode || ""}</p>
          <div class="flex flex-wrap gap-2 mt-2">
            {#if preview.mva_registrert}<span class="badge badge-ghost">MVA-registrert</span>{/if}
            {#if preview.autorisasjon.regnskap}
              <span class="badge badge-success">Autorisert regnskapsførerselskap</span>
            {/if}
            {#if preview.autorisasjon.revisjon}
              <span class="badge badge-success">Autorisert revisjonsselskap</span>
            {/if}
            {#if preview.konkurs}<span class="badge badge-error">Konkurs</span>{/if}
            {#if preview.slettet}<span class="badge badge-error">Slettet</span>{/if}
          </div>

          {#if sperret}
            <div class="alert alert-error mt-4 text-sm">
              En slettet eller konkurs enhet kan ikke registreres.
            </div>
          {:else if valg === "selskap"}
            {#if autorisert}
              <div class="alert alert-info mt-4 text-sm">
                <span>
                  Dette er et autorisert byrå. Skal dere føre regnskap for andre, registrer det som
                  byrå — selskapet kan i tillegg opprettes her for byråets eget regnskap.
                </span>
                <button class="btn btn-sm" onclick={() => velg("byra")}>Registrer som byrå</button>
              </div>
            {/if}
            <button class="btn btn-primary btn-sm mt-4" disabled={travelt} onclick={opprettSelskap}>
              Opprett selskap
            </button>
          {:else if !autorisert}
            <div class="alert alert-warning mt-4 text-sm">
              <span>
                Fant ingen aktiv autorisasjon i Finanstilsynets register for dette
                organisasjonsnummeret. Uten autorisasjon kan det ikke registreres som byrå — men det
                kan opprettes som vanlig selskap.
              </span>
              <button class="btn btn-sm" onclick={() => velg("selskap")}>Opprett som selskap</button>
            </div>
          {:else}
            {#if preview.autorisasjon.regnskap && preview.autorisasjon.revisjon}
              <div class="flex gap-4 mt-3">
                <label class="label cursor-pointer gap-2">
                  <input type="radio" class="radio radio-sm" bind:group={kind} value="regnskap" />
                  <span>Regnskapsførerselskap</span>
                </label>
                <label class="label cursor-pointer gap-2">
                  <input type="radio" class="radio radio-sm" bind:group={kind} value="revisjon" />
                  <span>Revisjonsselskap</span>
                </label>
              </div>
            {/if}
            <button class="btn btn-primary btn-sm mt-4" disabled={travelt} onclick={registrerByra}>
              Registrer byrå
            </button>
            <p class="text-xs opacity-60 mt-2">
              Gratis for byrået. Klientene deres får hver sin prøvetid og sitt eget abonnement.
            </p>
          {/if}
        </div>
      </div>
    {/if}
  </div>
{/if}
