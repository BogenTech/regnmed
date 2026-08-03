<script>
  // Plattformvisningen (docs/auth.md §8) — for systemadmin/support.
  // Stamdata på tvers av selskaper og byråer, ALDRI noen hovedbok.
  // Serveren logger hvert kall herfra og viser loggen til den det
  // gjaldt; banneret sier det, så ingen tror de ser usporet.
  import { api, post, send } from "../lib/api.js";
  import { me } from "../lib/me.svelte.js";
  import { logout } from "../lib/auth.svelte.js";
  import { toast } from "../lib/toast.svelte.js";
  import ThemeControls from "./ThemeControls.svelte";
  import Card from "./Card.svelte";

  const systemadmin = $derived(me.plattform?.rolle === "systemadmin");

  const FANER = $derived(
    [
      ["selskaper", "Selskaper"],
      ["byraer", "Byråer"],
      ["brukere", "Brukere"],
      ...(systemadmin
        ? [
            ["kunder", "Kunder"],
            ["medlemmer", "Plattformbrukere"],
          ]
        : []),
    ],
  );

  let fane = $state("selskaper");
  let sok = $state("");
  let rader = $state(null);

  async function last() {
    rader = null;
    const q = sok.trim() ? "?sok=" + encodeURIComponent(sok.trim()) : "";
    try {
      if (fane === "selskaper") rader = (await api("/platform/companies" + q)).selskaper;
      else if (fane === "byraer") rader = (await api("/platform/firms" + q)).byraer;
      else if (fane === "brukere") rader = (await api("/platform/users" + q)).brukere;
      else if (fane === "kunder") rader = (await api("/platform/customers" + q)).kunder;
      else if (fane === "medlemmer") rader = (await api("/platform/members")).medlemmer;
    } catch (error) {
      rader = [];
      toast(error.message, false);
    }
  }

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

    <div class="join mb-4">
      {#each FANER as [slug, navn] (slug)}
        <button
          class="btn btn-sm join-item {fane === slug ? 'btn-primary' : ''}"
          onclick={() => {
            fane = slug;
            sok = "";
          }}
        >
          {navn}
        </button>
      {/each}
    </div>

    {#if fane !== "medlemmer"}
      <form class="mb-4 flex gap-2" onsubmit={sokSubmit}>
        <input
          class="input input-sm input-bordered w-full max-w-xs"
          placeholder="Søk (navn eller orgnr)"
          bind:value={sok}
        />
        <button class="btn btn-sm">Søk</button>
      </form>
    {/if}

    {#if !rader}
      <span class="loading loading-spinner loading-lg"></span>
    {:else if fane === "selskaper"}
      <Card title="Selskaper">
        <table class="table table-sm">
          <thead><tr><th>Orgnr</th><th>Navn</th></tr></thead>
          <tbody>
            {#each rader as c (c.company_id)}
              <tr><td>{c.orgnr}</td><td>{c.name}</td></tr>
            {:else}
              <tr><td colspan="2" class="opacity-70">Ingen treff.</td></tr>
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
            <select class="select select-sm select-bordered" bind:value={tildel.slag}>
              <option value="selskap">selskap</option>
              <option value="byra">byrå</option>
            </select>
            <input
              class="input input-sm input-bordered w-32"
              placeholder="Orgnr"
              required
              bind:value={tildel.orgnr}
            />
            <select class="select select-sm select-bordered" bind:value={tildel.rolle}>
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
            class="input input-sm input-bordered"
            placeholder="E-post (må ha logget inn)"
            required
            bind:value={ny.epost}
          />
          <select class="select select-sm select-bordered" bind:value={ny.rolle}>
            <option value="support">support</option>
            <option value="systemadmin">systemadmin</option>
          </select>
          <input class="input input-sm input-bordered" type="date" required bind:value={ny.valid_to} />
          <input
            class="input input-sm input-bordered w-56"
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
    {/if}
  {/if}
</main>
