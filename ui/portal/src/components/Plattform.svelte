<script>
  // Plattformvisningen (docs/auth.md §8) — for systemadmin/support.
  // Stamdata på tvers av selskaper og byråer, ALDRI noen hovedbok.
  // Serveren logger hvert kall herfra og viser loggen til den det
  // gjaldt; banneret sier det, så ingen tror de ser usporet.
  import { api, post, send } from "../lib/api.js";
  import { me } from "../lib/me.svelte.js";
  import { session, logout } from "../lib/auth.svelte.js";
  import { toast } from "../lib/toast.svelte.js";
  import { IKONSTILER } from "../lib/ikoner.js";
  import { prefs, setIkonstil } from "../lib/prefs.svelte.js";
  import ThemeControls from "./ThemeControls.svelte";
  import Card from "./Card.svelte";
  import Ikon from "./Ikon.svelte";
  import PlattformSelskap from "./PlattformSelskap.svelte";

  const systemadmin = $derived(me.plattform?.rolle === "systemadmin");

  // [slug, navn, ikon] — ikonet gjør fanene gjenkjennbare på et blikk,
  // samme faste tilordning som selskapsmenyen (lib/ikoner.js).
  const FANER = $derived(
    [
      ["oversikt", "Oversikt", "oversikt"],
      ["selskaper", "Selskaper", "selskaper"],
      ["byraer", "Byråer", "byraer"],
      ["brukere", "Brukere", "brukere"],
      ...(systemadmin
        ? [
            ["abonnementer", "Abonnementer", "abonnementer"],
            ["kunder", "Kunder", "kunder"],
            ["medlemmer", "Plattformbrukere", "medlemmer"],
            ["innstillinger", "Innstillinger", "admin"],
          ]
        : []),
    ],
  );

  let fane = $state("oversikt");
  let sok = $state("");
  let rader = $state(null);
  // Drill-down: ett valgt selskap i Selskaper-fanen.
  let valgtSelskap = $state(null);

  async function last() {
    rader = null;
    const q = sok.trim() ? "?sok=" + encodeURIComponent(sok.trim()) : "";
    try {
      if (fane === "oversikt") rader = await api("/platform/overview");
      else if (fane === "selskaper") rader = (await api("/platform/companies" + q)).selskaper;
      else if (fane === "byraer") rader = (await api("/platform/firms" + q)).byraer;
      else if (fane === "brukere") rader = (await api("/platform/users" + q)).brukere;
      else if (fane === "abonnementer")
        rader = (await api("/platform/subscriptions")).abonnementer;
      else if (fane === "kunder") rader = (await api("/platform/customers" + q)).kunder;
      else if (fane === "medlemmer") rader = (await api("/platform/members")).medlemmer;
      else if (fane === "innstillinger") rader = await api("/platform/settings");
    } catch (error) {
      rader = [];
      toast(error.message, false);
    }
  }

  async function lagreIkonstil(stil) {
    try {
      await send("PUT", "/platform/settings", { ikonstil: stil });
      // Oppdater visningen straks — alle andre får den ved neste
      // innlasting av portal-config.
      setIkonstil(stil);
      toast("Ikonstilen er satt for hele plattformen", true);
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Samme fargespråk som abonnementskortet: statusen ser lik ut hos
  // kunden og i konsollen.
  const ABO_FARGE = {
    aktiv: "badge-success",
    prove: "badge-info",
    frist: "badge-warning",
    sperret: "badge-error",
  };
  const ABO_TEKST = { aktiv: "aktiv", prove: "prøvetid", frist: "ubetalt", sperret: "sperret" };

  $effect(() => {
    fane;
    last();
  });

  function sokSubmit(event) {
    event.preventDefault();
    last();
  }

  // Tildeling av medlemskap: orgnr peker ut selskapet/byrået, og løses
  // gjennom det samme søket som listene bruker — bare et EKSAKT
  // orgnr-treff godtas.
  let tildel = $state(null); // { person_id, navn, slag, orgnr, rolle }

  function startTildeling(bruker) {
    tildel = { person_id: bruker.person_id, navn: bruker.navn, slag: "selskap", orgnr: "", rolle: "les" };
  }

  const ROLLER = { selskap: ["les", "bokforing", "ansatt", "admin"], byra: ["ansatt", "admin"] };

  async function utfoerTildeling(event) {
    event.preventDefault();
    try {
      const sti = tildel.slag === "selskap" ? "/platform/companies" : "/platform/firms";
      const treffliste = await api(sti + "?sok=" + encodeURIComponent(tildel.orgnr.trim()));
      const liste = treffliste.selskaper || treffliste.byraer || [];
      const treff = liste.find((t) => t.orgnr === tildel.orgnr.trim());
      if (!treff) throw new Error("ingen " + tildel.slag + " med orgnr " + tildel.orgnr);
      const id = treff.company_id || treff.firm_id;
      const del = tildel.slag === "selskap" ? "/companies/" : "/firms/";
      await post("/platform/users/" + tildel.person_id + del + id, { rolle: tildel.rolle });
      toast("Tilgang tildelt — handlingen er logget hos " + treff.name, true);
      tildel = null;
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Plattformbrukere (systemadmin): tildel og avslutt roller.
  let ny = $state({ epost: "", rolle: "support", valid_to: "", notat: "" });

  async function giRolle(event) {
    event.preventDefault();
    try {
      await post("/platform/members", ny);
      toast("Plattformrolle tildelt", true);
      ny = { epost: "", rolle: "support", valid_to: "", notat: "" };
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function avslutt(m) {
    try {
      await send("DELETE", "/platform/members/" + m.id);
      toast("Medlemskapet er avsluttet med øyeblikkelig virkning", true);
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<div class="navbar bg-base-100 shadow-sm">
  <div class="flex-1">
    <a class="btn btn-ghost text-xl" href="#/">regnmed</a>
    <span class="badge badge-warning">Plattform · {me.plattform?.rolle}</span>
  </div>
  <div class="flex-none gap-2">
    <ThemeControls />
    <button class="btn btn-ghost btn-sm" onclick={logout}>Logg ut</button>
  </div>
</div>

<main class="p-6 max-w-5xl mx-auto">
  {#if !me.plattform}
    <div class="alert alert-warning"><span>Du har ingen aktiv plattformrolle.</span></div>
  {:else}
    <div class="alert alert-info text-sm mb-4">
      <span>
        Alle kall her logges og er synlige for selskapet eller byrået de gjelder
        (docs/auth.md §8). Ingen plattformrolle når noe selskaps regnskap.
        Rollen din utløper {me.plattform.valid_to}.
      </span>
    </div>

    <div role="tablist" class="tabs tabs-box tabs-sm mb-4 flex-wrap">
      {#each FANER as [slug, navn, ikonNavn] (slug)}
        <button
          role="tab"
          class="tab gap-1.5 {fane === slug ? 'tab-active' : ''}"
          aria-selected={fane === slug}
          onclick={() => {
            fane = slug;
            sok = "";
            valgtSelskap = null;
          }}
        >
          <Ikon navn={ikonNavn} />
          {navn}
        </button>
      {/each}
    </div>

    {#if fane === "selskaper" && valgtSelskap}
      <PlattformSelskap
        companyId={valgtSelskap}
        {systemadmin}
        onClose={() => {
          valgtSelskap = null;
          last();
        }}
      />
    {:else}
    {#if fane !== "medlemmer" && fane !== "oversikt" && fane !== "abonnementer" && fane !== "innstillinger"}
      <form class="mb-4 flex gap-2" onsubmit={sokSubmit}>
        <input
          class="input input-sm w-full max-w-xs"
          placeholder="Søk (navn eller orgnr)"
          bind:value={sok}
        />
        <button class="btn btn-sm">Søk</button>
      </form>
    {/if}

    {#if !rader}
      <span class="loading loading-spinner loading-lg"></span>
    {:else if fane === "oversikt"}
      <!-- Dashbordet: administrative tall + abonnementsfordelingen.
           Rene aggregater fra /platform/overview — ingen hovedbok. -->
      <div class="stats shadow-sm bg-base-100 w-full mb-6 stats-vertical sm:stats-horizontal">
        {#each [["selskaper", "Selskaper", rader.selskaper], ["byraer", "Byråer", rader.byraer], ["brukere", "Brukere", rader.brukere], ["medlemmer", "Plattformbrukere", rader.plattformbrukere]] as [slug, tittel, verdi] (slug)}
          <button
            class="stat text-left cursor-pointer"
            onclick={() => {
              if (FANER.some(([f]) => f === slug)) fane = slug;
            }}
          >
            <div class="stat-figure opacity-60"><Ikon navn={slug} /></div>
            <div class="stat-title">{tittel}</div>
            <div class="stat-value text-2xl">{verdi}</div>
          </button>
        {/each}
        <div class="stat">
          <div class="stat-title">Integrasjoner</div>
          <div class="stat-value text-2xl">{rader.integrasjoner}</div>
          <div class="stat-desc">maskintilganger</div>
        </div>
      </div>

      <Card title="Abonnementer">
        <div class="flex gap-2 flex-wrap">
          {#each Object.entries(rader.abonnement) as [slug, antall] (slug)}
            <span class="badge {ABO_FARGE[slug] || 'badge-ghost'}">
              {ABO_TEKST[slug] || slug}: {antall}
            </span>
          {:else}
            <span class="opacity-70">Ingen selskaper ennå.</span>
          {/each}
        </div>
        {#if systemadmin}
          <p class="text-sm mt-2">
            <button class="link" onclick={() => (fane = "abonnementer")}>
              Se per selskap →
            </button>
          </p>
        {/if}
      </Card>
    {:else if fane === "abonnementer"}
      <Card title="Abonnementer per selskap">
        <p class="text-sm opacity-70 mb-2">
          Status beregnes av samme regel som hos kunden (aldri lagret). Faktureringen bor i
          driftsselskapets hovedbok — dette er kundeforholdet, ikke saldoen.
        </p>
        <table class="table table-sm">
          <thead>
            <tr><th>Orgnr</th><th>Selskap</th><th>Plan</th><th>Status</th><th>Dato</th><th>Opprettet</th></tr>
          </thead>
          <tbody>
            {#each rader as a (a.company_id)}
              <tr>
                <td>{a.orgnr}</td>
                <td>{a.name}</td>
                <td>{a.plan || "–"}</td>
                <td>
                  <span class="badge badge-sm {ABO_FARGE[a.status] || 'badge-ghost'}">
                    {ABO_TEKST[a.status] || a.status}
                  </span>
                </td>
                <td>{a.dato || ""}</td>
                <td>{a.opprettet}</td>
              </tr>
            {:else}
              <tr><td colspan="6" class="opacity-70">Ingen selskaper.</td></tr>
            {/each}
          </tbody>
        </table>
      </Card>
    {:else if fane === "selskaper"}
      <Card title="Selskaper">
        <table class="table table-sm">
          <thead><tr><th>Orgnr</th><th>Navn</th><th></th></tr></thead>
          <tbody>
            {#each rader as c (c.company_id)}
              <tr>
                <td>{c.orgnr}</td>
                <td>{c.name}</td>
                <td>
                  <button
                    class="btn btn-ghost btn-xs"
                    onclick={() => (valgtSelskap = c.company_id)}
                  >
                    Åpne
                  </button>
                </td>
              </tr>
            {:else}
              <tr><td colspan="3" class="opacity-70">Ingen treff.</td></tr>
            {/each}
          </tbody>
        </table>
      </Card>
    {:else if fane === "byraer"}
      <Card title="Byråer">
        <table class="table table-sm">
          <thead><tr><th>Orgnr</th><th>Navn</th><th>Type</th></tr></thead>
          <tbody>
            {#each rader as f (f.firm_id)}
              <tr><td>{f.orgnr}</td><td>{f.name}</td><td>{f.kind}</td></tr>
            {:else}
              <tr><td colspan="3" class="opacity-70">Ingen treff.</td></tr>
            {/each}
          </tbody>
        </table>
      </Card>
    {:else if fane === "brukere"}
      <Card title="Brukere">
        <p class="text-sm opacity-70 mb-2">
          Navn og e-post eies av innloggingstjenesten
          {#if session.config?.issuer}
            (<a class="link" href={session.config.issuer} target="_blank" rel="noreferrer">
              {session.config.issuer.replace(/^https?:\/\//, "")}</a>)
          {:else}
            (IdP-en)
          {/if}
          og speiles hit ved innlogging — identitetsdata endres der, tilganger her.
        </p>
        <table class="table table-sm">
          <thead>
            <tr><th>Navn</th><th>E-post</th><th>Tilknytninger</th><th></th></tr>
          </thead>
          <tbody>
            {#each rader as b (b.person_id)}
              <tr>
                <td>{b.navn}{b.kind === "integrasjon" ? " 🤖" : ""}</td>
                <td>{b.epost || ""}</td>
                <td>
                  {#each b.tilknytninger as t, i (i)}
                    <span class="badge badge-ghost badge-sm mr-1 {t.aktiv ? '' : 'line-through'}">
                      {t.slag === "byra" ? "byrå " : ""}{t.navn}: {t.rolle}
                    </span>
                  {:else}
                    <span class="opacity-50">ingen</span>
                  {/each}
                </td>
                <td>
                  {#if b.kind === "menneske"}
                    <button class="btn btn-ghost btn-xs" onclick={() => startTildeling(b)}>
                      Tildel tilgang
                    </button>
                  {/if}
                </td>
              </tr>
            {:else}
              <tr><td colspan="4" class="opacity-70">Ingen treff.</td></tr>
            {/each}
          </tbody>
        </table>
        {#if tildel}
          <form class="mt-3 flex flex-wrap gap-2 items-end" onsubmit={utfoerTildeling}>
            <span class="text-sm self-center">Tildel {tildel.navn} tilgang til</span>
            <select class="select select-sm" bind:value={tildel.slag}>
              <option value="selskap">selskap</option>
              <option value="byra">byrå</option>
            </select>
            <input
              class="input input-sm w-32"
              placeholder="Orgnr"
              required
              bind:value={tildel.orgnr}
            />
            <select class="select select-sm" bind:value={tildel.rolle}>
              {#each ROLLER[tildel.slag] as r (r)}
                <option value={r}>{r}</option>
              {/each}
            </select>
            <button class="btn btn-sm btn-primary">Tildel</button>
            <button type="button" class="btn btn-sm btn-ghost" onclick={() => (tildel = null)}>
              Avbryt
            </button>
            {#if !systemadmin}
              <span class="text-xs opacity-70">
                Support kan bare tildele NYE medlemskap — endringer krever systemadmin.
              </span>
            {/if}
          </form>
        {/if}
      </Card>
    {:else if fane === "kunder"}
      <Card title="Kunder (alle selskaper)">
        <p class="text-sm opacity-70 mb-2">
          Stamdata med eierselskapet navngitt — aldri saldoer. Kundens tilknytning
          til sitt selskap er fast og kan ikke flyttes.
        </p>
        <table class="table table-sm">
          <thead>
            <tr><th>Nr</th><th>Navn</th><th>Orgnr</th><th>E-post</th><th>Selskap</th></tr>
          </thead>
          <tbody>
            {#each rader as k (k.party_id)}
              <tr>
                <td>{k.party_no}</td>
                <td>{k.navn}</td>
                <td>{k.orgnr || ""}</td>
                <td>{k.epost || ""}</td>
                <td>{k.selskap.navn} ({k.selskap.orgnr})</td>
              </tr>
            {:else}
              <tr><td colspan="5" class="opacity-70">Ingen treff.</td></tr>
            {/each}
          </tbody>
        </table>
      </Card>
    {:else if fane === "medlemmer"}
      <Card title="Plattformbrukere">
        <form class="flex flex-wrap gap-2 items-end mb-4" onsubmit={giRolle}>
          <input
            class="input input-sm"
            placeholder="E-post (må ha logget inn)"
            required
            bind:value={ny.epost}
          />
          <select class="select select-sm" bind:value={ny.rolle}>
            <option value="support">support</option>
            <option value="systemadmin">systemadmin</option>
          </select>
          <input class="input input-sm" type="date" required bind:value={ny.valid_to} />
          <input
            class="input input-sm w-56"
            placeholder="Begrunnelse (obligatorisk)"
            required
            bind:value={ny.notat}
          />
          <button class="btn btn-sm btn-primary">Gi rolle</button>
        </form>
        <table class="table table-sm">
          <thead>
            <tr><th>Navn</th><th>Rolle</th><th>Gyldig</th><th>Begrunnelse</th><th></th></tr>
          </thead>
          <tbody>
            {#each rader as m (m.id)}
              <tr class={m.aktiv ? "" : "opacity-50"}>
                <td>{m.navn} {m.epost ? "(" + m.epost + ")" : ""}</td>
                <td>{m.rolle}</td>
                <td>{m.valid_from} → {m.valid_to}</td>
                <td>{m.notat}</td>
                <td>
                  {#if m.aktiv}
                    <button class="btn btn-ghost btn-xs" onclick={() => avslutt(m)}>Avslutt</button>
                  {/if}
                </td>
              </tr>
            {:else}
              <tr><td colspan="5" class="opacity-70">Ingen plattformbrukere.</td></tr>
            {/each}
          </tbody>
        </table>
      </Card>
    {:else if fane === "innstillinger"}
      <Card title="Innstillinger — hele plattformen">
        <p class="text-sm opacity-70 mb-3">
          Ikonstilen i portalmenyen, låst globalt for alle brukere. Endringen gjelder ved neste
          innlasting; din egen visning oppdateres straks.
        </p>
        <div class="flex flex-col gap-2 max-w-md">
          {#each IKONSTILER as [slug, navn] (slug)}
            <label
              class="flex items-center gap-3 rounded-box border border-base-300 px-3 py-2 cursor-pointer
                     {prefs.ikonstil === slug ? 'border-primary bg-base-200' : ''}"
            >
              <input
                type="radio"
                name="ikonstil"
                class="radio radio-sm"
                value={slug}
                checked={prefs.ikonstil === slug}
                onchange={() => lagreIkonstil(slug)}
              />
              <span class="w-28">{navn}</span>
              <span class="flex items-center gap-3 opacity-80">
                {#if slug === "ingen"}
                  <span class="text-xs opacity-60">bare tekst</span>
                {:else}
                  {#each ["oversikt", "faktura", "bank", "rapporter", "timer"] as ikonNavn (ikonNavn)}
                    <Ikon navn={ikonNavn} stil={slug} />
                  {/each}
                {/if}
              </span>
            </label>
          {/each}
        </div>
      </Card>
    {/if}
    {/if}
  {/if}
</main>
