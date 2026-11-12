<script>
  // Ny kunde eller leverandør: starter med organisasjonsnummeret, og
  // Enhetsregisteret fyller inn navn og adresse. E-post og kontonummer
  // tas i samme skjema — for en kunde så fakturaen kan sendes med en
  // gang, for en leverandør så den kan betales. Privatpersoner har ikke
  // orgnr; da skrives navnet.
  import { untrack } from "svelte";
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, kind = "kunde" } = $props();

  const T = {
    kunde: {
      section: "kunder", title: "Ny kunde", all: "← Alle kunder", one: "kunde", submit: "Legg til kunde",
      already: "er allerede kunde", open: "Åpne kunden", emailHelp: "Fakturaer sendes hit",
      accountLabel: "Kontonummer (valgfritt)", accountHelp: "Bare hvis dere skal betale penger tilbake",
      warn: "Tenk deg om før du fakturerer.",
    },
    leverandor: {
      section: "leverandorer", title: "Ny leverandør", all: "← Alle leverandører", one: "leverandør",
      submit: "Legg til leverandør", already: "er allerede leverandør", open: "Åpne leverandøren",
      emailHelp: "For kontakt om fakturaer", accountLabel: "Kontonummer",
      accountHelp: "Dit dere betaler. Trengs for å betale fra betalingslisten.",
      warn: "Sjekk at det er riktig selskap før dere betaler.",
    },
  }[untrack(() => kind)];

  let orgnr = $state("");
  let name = $state("");
  let address = $state("");
  let email = $state("");
  let bankAccount = $state("");
  let lookup = $state(null); // { status: "loading" | "found" | "error", unit?, message? }
  let existing = $state(null);
  let submitted = $state(false);
  let busy = $state(false);

  let digits = $derived(orgnr.replace(/\s/g, ""));

  // Look up as soon as nine digits are typed. Fields the user already
  // typed are kept; empty ones are filled from the register.
  let lastLooked = "";
  $effect(() => {
    const nr = digits;
    if (!/^\d{9}$/.test(nr) || nr === lastLooked) {
      if (!nr) lookup = existing = null;
      return;
    }
    lastLooked = nr;
    lookup = { status: "loading" };
    existing = null;
    api("/registry/enheter/" + nr + "?authorisations=false")
      .then((unit) => {
        if (digits !== nr) return;
        lookup = { status: "found", unit };
        if (!name.trim()) name = unit.navn || "";
        if (!address.trim()) address = unit.adresse || "";
      })
      .catch((e) => digits === nr && (lookup = { status: "error", message: e.message }));
    // The search also matches names; only the same orgnr is a duplicate.
    api("/companies/" + companyId + "/parties?kind=" + kind + "&limit=10&q=" + nr)
      .then((r) => {
        if (digits !== nr) return;
        existing = r.parties.find((p) => (p.orgnr || "").replace(/\s/g, "") === nr) || null;
      })
      .catch(() => {});
  });

  let errors = $derived({
    orgnr: digits && !/^\d{9}$/.test(digits) ? "Organisasjonsnummer har 9 siffer" : null,
    name: !name.trim() ? "Skriv kundens navn" : null,
    email: email.trim() && !email.includes("@") ? "Skriv en hel e-postadresse" : null,
  });
  const err = (field) => (submitted ? errors[field] : null);

  async function create(event) {
    event.preventDefault();
    submitted = true;
    const first = Object.entries(errors).find(([, v]) => v);
    if (first) {
      document.getElementById("nc-" + first[0])?.focus();
      return;
    }
    busy = true;
    try {
      const created = await post("/companies/" + companyId + "/parties", {
        kind,
        name: name.trim(),
        orgnr: digits || null,
        address: address.trim() || null,
        email: email.trim() || null,
        bank_account: bankAccount.trim() || null,
      });
      toast(name.trim() + " er lagt til som " + T.one + " " + created.party_no, true);
      location.hash = "#/c/" + companyId + "/" + T.section + "/" + created.party_id;
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }
</script>

<nav class="mb-3">
  <a class="link link-hover text-sm" href={"#/c/" + companyId + "/" + T.section}>{T.all}</a>
</nav>
<h1 class="text-2xl font-bold mb-4">{T.title}</h1>

<form class="card border border-base-300 bg-base-100 max-w-2xl" novalidate onsubmit={create}>
  <div class="card-body grid gap-4 sm:grid-cols-2">
    <label class="fieldset sm:col-span-2">
      <span class="fieldset-legend">Organisasjonsnummer</span>
      <input
        id="nc-orgnr"
        class="input w-full sm:w-60 {err('orgnr') ? 'input-error' : ''}"
        inputmode="numeric"
        autocomplete="off"
        bind:value={orgnr}
        aria-describedby="nc-orgnr-help"
        aria-invalid={!!err("orgnr")}
      />
      <span id="nc-orgnr-help" class="label whitespace-normal" aria-live="polite">
        {#if err("orgnr")}
          <span class="text-error">{err("orgnr")}</span>
        {:else if lookup?.status === "loading"}
          <span class="loading loading-dots loading-xs"></span> Henter fra Enhetsregisteret
        {:else if lookup?.status === "found"}
          Hentet fra Enhetsregisteret{lookup.unit.mva_registrert ? ", mva-registrert" : ""}
        {:else if lookup?.status === "error"}
          <span class="text-error">{lookup.message}</span>
        {:else}
          Navn og adresse hentes fra Enhetsregisteret. Privatperson? La feltet stå tomt.
        {/if}
      </span>
    </label>

    {#if lookup?.status === "found" && (lookup.unit.konkurs || lookup.unit.under_avvikling || lookup.unit.slettet)}
      <div class="alert alert-warning sm:col-span-2" role="alert">
        <span>
          {lookup.unit.navn} er {lookup.unit.slettet ? "slettet" : lookup.unit.konkurs ? "konkurs" : "under avvikling"}
          ifølge Enhetsregisteret. {T.warn}
        </span>
      </div>
    {/if}
    {#if existing}
      <div class="alert alert-info sm:col-span-2">
        <span>{existing.name} {T.already} nr. {existing.party_no}.</span>
        <a class="btn btn-sm" href={"#/c/" + companyId + "/" + T.section + "/" + existing.party_id}>{T.open}</a>
      </div>
    {/if}

    <label class="fieldset sm:col-span-2">
      <span class="fieldset-legend">Navn</span>
      <input
        id="nc-name"
        class="input w-full {err('name') ? 'input-error' : ''}"
        autocomplete="organization"
        bind:value={name}
        aria-invalid={!!err("name")}
        aria-describedby="nc-name-help"
      />
      {#if err("name")}<span id="nc-name-help" class="label text-error">{err("name")}</span>{/if}
    </label>
    <label class="fieldset sm:col-span-2">
      <span class="fieldset-legend">Adresse</span>
      <input class="input w-full" autocomplete="street-address" bind:value={address} />
      <span class="label whitespace-normal">Står på fakturaen</span>
    </label>
    <label class="fieldset">
      <span class="fieldset-legend">E-post</span>
      <input
        id="nc-email"
        class="input w-full {err('email') ? 'input-error' : ''}"
        type="email"
        autocomplete="email"
        bind:value={email}
        aria-invalid={!!err("email")}
        aria-describedby="nc-email-help"
      />
      <span id="nc-email-help" class="label whitespace-normal {err('email') ? 'text-error' : ''}">
        {err("email") || T.emailHelp}
      </span>
    </label>
    <label class="fieldset">
      <span class="fieldset-legend">{T.accountLabel}</span>
      <input class="input w-full" inputmode="numeric" bind:value={bankAccount} />
      <span class="label whitespace-normal">{T.accountHelp}</span>
    </label>

    <div class="flex flex-wrap gap-2 sm:col-span-2">
      <button class="btn btn-primary" disabled={busy}>
        {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
        {T.submit}
      </button>
      <a class="btn btn-ghost" href={"#/c/" + companyId + "/" + T.section}>Avbryt</a>
    </div>
  </div>
</form>
