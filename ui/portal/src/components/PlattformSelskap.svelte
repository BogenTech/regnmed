<script>
  // Back-office drill-down for one company (docs/auth.md §8): master
  // data, memberships, invitations and the abonnement relationship on
  // one page. Support ser, systemadmin endrer — serveren avgjør per
  // handling, og hvert kall logges og er synlig for selskapet.
  import { api, post, send } from "../lib/api.js";
  import { toast } from "../lib/toast.svelte.js";
  import Card from "./Card.svelte";

  let { companyId, systemadmin, onClose } = $props();

  let data = $state(null);
  let skjema = $state(null);

  async function last() {
    try {
      data = await api("/platform/companies/" + companyId);
      skjema = {
        address: data.settings.address || "",
        email: data.settings.email || "",
        bank_account: data.settings.bank_account || "",
        orgform: data.settings.orgform || "",
      };
    } catch (error) {
      toast(error.message, false);
      onClose();
    }
  }
  last();

  const ORGFORMER = ["", "AS", "ASA", "ENK", "ANS", "DA"];
  const ROLLER = ["les", "bokforing", "ansatt", "admin"];

  const ABO_FARGE = {
    aktiv: "badge-success",
    prove: "badge-info",
    frist: "badge-warning",
    sperret: "badge-error",
  };

  async function lagreSettings(event) {
    event.preventDefault();
    try {
      await send("PUT", "/platform/companies/" + companyId + "/settings", skjema);
      toast("Firmaopplysninger lagret — handlingen er logget", true);
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function settRolle(m, rolle) {
    try {
      await post("/platform/users/" + m.person_id + "/companies/" + companyId, { rolle });
      toast("Rolle endret — logget hos selskapet", true);
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function deaktiver(m) {
    if (!confirm("Deaktivere tilgangen til " + m.navn + "?")) return;
    try {
      await send("DELETE", "/platform/companies/" + companyId + "/members/" + m.person_id);
      toast("Deaktivert — logget hos selskapet", true);
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function gjenopprett(m) {
    try {
      await post("/platform/companies/" + companyId + "/members/" + m.person_id + "/restore", {});
      toast("Gjenopprettet", true);
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }

  let nyDekning = $state({ plan: "standard", note: "" });
  let harApenDekning = $derived(!!data?.abonnement.dekning.find((d) => !d.valid_to));

  async function startDekning(event) {
    event.preventDefault();
    try {
      await post("/platform/companies/" + companyId + "/subscription", nyDekning);
      toast("Dekning åpnet", true);
      nyDekning = { plan: "standard", note: "" };
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function avsluttDekning() {
    if (!confirm("Avslutte den åpne dekningen? Vanlig frist løper før noe sperres.")) return;
    try {
      await post("/platform/companies/" + companyId + "/subscription/end", {});
      toast("Dekningen er avsluttet", true);
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <div class="flex items-baseline gap-3 mb-4">
    <button class="btn btn-sm btn-ghost" onclick={onClose}>← Selskaper</button>
    <h2 class="text-lg font-semibold">{data.settings.name}</h2>
    <span class="opacity-60 text-sm">{data.settings.orgnr}</span>
    <span class="badge badge-sm {ABO_FARGE[data.abonnement.status] || 'badge-ghost'}">
      {data.abonnement.status}{data.abonnement.dato ? " · " + data.abonnement.dato : ""}
    </span>
  </div>

  <Card title="Firmaopplysninger">
    <form class="grid gap-2 max-w-md" onsubmit={lagreSettings}>
      <input
        class="input input-sm"
        placeholder="Adresse"
        bind:value={skjema.address}
        disabled={!systemadmin}
      />
      <input
        class="input input-sm"
        placeholder="E-post"
        bind:value={skjema.email}
        disabled={!systemadmin}
      />
      <div class="flex gap-2">
        <input
          class="input input-sm flex-1"
          placeholder="Kontonummer"
          bind:value={skjema.bank_account}
          disabled={!systemadmin}
        />
        <select class="select select-sm" bind:value={skjema.orgform} disabled={!systemadmin}>
          {#each ORGFORMER as f (f)}
            <option value={f}>{f || "(selskapsform)"}</option>
          {/each}
        </select>
      </div>
      {#if systemadmin}
        <button class="btn btn-sm">Lagre</button>
      {:else}
        <span class="text-xs opacity-60">Endring krever systemadmin.</span>
      {/if}
    </form>
  </Card>

  <Card title="Medlemmer">
    <table class="table table-sm">
      <thead>
        <tr><th>Navn</th><th>E-post</th><th>Rolle</th><th>Via</th><th></th></tr>
      </thead>
      <tbody>
        {#each data.medlemmer as m (m.person_id)}
          <tr class={m.aktiv ? "" : "opacity-50"}>
            <td>{m.navn}</td>
            <td class="text-xs">{m.epost || ""}</td>
            <td>
              {#if systemadmin && m.kan_endres && m.aktiv}
                <select
                  class="select select-xs"
                  value={m.rolle}
                  onchange={(e) => settRolle(m, e.target.value)}
                >
                  {#each ROLLER.includes(m.rolle) ? ROLLER : [m.rolle, ...ROLLER] as r (r)}
                    <option value={r}>{r}</option>
                  {/each}
                </select>
              {:else}
                {m.rolle}
              {/if}
            </td>
            <td class="text-xs opacity-70">{m.via}</td>
            <td>
              {#if systemadmin && m.kan_endres}
                {#if m.aktiv}
                  <button class="btn btn-ghost btn-xs" onclick={() => deaktiver(m)}>
                    Deaktiver
                  </button>
                {:else}
                  <button class="btn btn-ghost btn-xs" onclick={() => gjenopprett(m)}>
                    Gjenopprett
                  </button>
                {/if}
              {/if}
            </td>
          </tr>
        {:else}
          <tr><td colspan="5" class="opacity-70">Ingen direkte medlemmer.</td></tr>
        {/each}
      </tbody>
    </table>
    <p class="text-xs opacity-60 mt-1">
      Alt her logges med kilde «plattform» i selskapets egen endringslogg. Tilgang via oppdrag
      styres av oppdraget.
    </p>
  </Card>

  {#if data.invitasjoner.length}
    <Card title="Åpne invitasjoner">
      <table class="table table-sm">
        <thead><tr><th>E-post</th><th>Rolle</th><th>Sist sendt</th></tr></thead>
        <tbody>
          {#each data.invitasjoner as i (i.id)}
            <tr>
              <td>{i.epost}</td>
              <td>{i.rolle}</td>
              <td class="text-xs opacity-70">
                {i.sist_sendt ? new Date(i.sist_sendt).toLocaleString("no") : "ikke sendt"}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </Card>
  {/if}

  <Card title="Abonnement">
    <table class="table table-sm mb-2">
      <thead>
        <tr><th>Plan</th><th>Fra</th><th>Til</th><th>Referanse</th><th>Av</th></tr>
      </thead>
      <tbody>
        {#each data.abonnement.dekning as d, i (i)}
          <tr>
            <td>{d.plan}</td>
            <td>{d.valid_from}</td>
            <td>{d.valid_to || "løpende"}</td>
            <td class="text-xs">{d.note}</td>
            <td class="text-xs opacity-70">{d.created_by}</td>
          </tr>
        {:else}
          <tr><td colspan="5" class="opacity-70">Ingen dekningsrader — selskapet er i prøvetid.</td></tr>
        {/each}
      </tbody>
    </table>
    {#if systemadmin}
      {#if harApenDekning}
        <button class="btn btn-sm btn-outline btn-error" onclick={avsluttDekning}>
          Avslutt åpen dekning
        </button>
      {:else}
        <form class="flex gap-2 flex-wrap items-end" onsubmit={startDekning}>
          <select class="select select-sm" bind:value={nyDekning.plan}>
            <option value="basis">basis</option>
            <option value="standard">standard</option>
          </select>
          <input
            class="input input-sm w-72"
            placeholder="Referanse (obligatorisk — hvorfor åpnes dekningen)"
            required
            bind:value={nyDekning.note}
          />
          <button class="btn btn-sm btn-primary">Åpne dekning</button>
        </form>
      {/if}
    {/if}
  </Card>
{/if}
