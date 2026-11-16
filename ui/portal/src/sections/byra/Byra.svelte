<script>
  // Byråvisningen er ikke en selskapsseksjon: den står utenfor et
  // enkelt selskap og har derfor sitt eget skall uten selskapsmeny.
  //
  // The firm's own entry in /firms/mine decides what is fetched: member
  // administration and the vendor-access log are admin territory (#78,
  // docs/auth.md §8), and a firm this person is not in is said plainly
  // instead of spinning. A failed request offers a retry.
  import { api } from "../../lib/api.js";
  import UserMenu from "../../components/UserMenu.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import PlattformInnsyn from "../brukere/PlattformInnsyn.svelte";
  import Foresporsler from "./Foresporsler.svelte";
  import Klienter from "./Klienter.svelte";
  import Medlemmer from "./Medlemmer.svelte";

  let { firmId } = $props();

  let data = $state(null);
  let feil = $state(null);

  async function load() {
    const id = firmId;
    try {
      const mine = await api("/firms/mine");
      const firm = mine.firms.find((f) => f.firm_id === id) || null;
      if (!firm) {
        if (id === firmId) data = { firm: null };
        return;
      }
      const admin = firm.role === "admin";
      const hent = (ok, sti) => (ok ? api("/firms/" + id + sti) : Promise.resolve(null));
      const [requests, clients, access, invitations, innsyn] = await Promise.all([
        hent(true, "/requests"),
        hent(true, "/clients"),
        hent(admin, "/access"),
        hent(admin, "/invitations"),
        hent(admin, "/platform-access"),
      ]);
      if (id !== firmId) return;
      feil = null;
      data = {
        firm,
        admin,
        pending: requests.requests.filter((r) => r.status === "pending"),
        clients: clients.clients,
        medlemmer: access?.medlemmer || [],
        invitasjoner: invitations?.invitasjoner || [],
        innsyn: innsyn?.innsyn || null,
      };
    } catch (error) {
      if (id === firmId) feil = error.message;
    }
  }

  $effect(() => {
    void firmId;
    data = null;
    feil = null;
    load();
  });
</script>

<div class="navbar bg-base-100 shadow-sm">
  <div class="flex-1 gap-2 items-baseline min-w-0">
    <a href="#/" class="btn btn-ghost text-xl">regnmed</a>
    <span class="text-sm opacity-70 truncate">{data?.firm?.name || ""}</span>
  </div>
  <div class="flex-none gap-2">
    <UserMenu />
  </div>
</div>
<main class="p-4 sm:p-6 max-w-4xl mx-auto">
  <div class="breadcrumbs text-sm pt-0 pb-3">
    <ul>
      <li><a href="#/">Mine selskaper</a></li>
      <li>{data?.firm?.name || "Byrå"}</li>
    </ul>
  </div>
  {#if feil}
    <LoadError error={feil} retry={load} />
  {:else if !data}
    <span class="loading loading-spinner loading-lg" aria-label="Laster byrået"></span>
  {:else if !data.firm}
    <div class="alert alert-warning">
      <span>
        Du er ikke medlem av dette byrået, eller tilgangen er avsluttet.
        <a class="link" href="#/">Tilbake til Mine selskaper</a>
      </span>
    </div>
  {:else}
    <Foresporsler {firmId} pending={data.pending} admin={data.admin} onDone={load} />
    <Klienter clients={data.clients} />
    {#if data.admin}
      <Medlemmer
        {firmId}
        medlemmer={data.medlemmer}
        invitasjoner={data.invitasjoner}
        onDone={load}
      />
      {#if data.innsyn}<PlattformInnsyn innsyn={data.innsyn} hvor="byrået" />{/if}
    {/if}
  {/if}
</main>
