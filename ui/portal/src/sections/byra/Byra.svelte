<script>
  // Byråvisningen er ikke en selskapsseksjon: den står utenfor et
  // enkelt selskap og har derfor sitt eget skall uten selskapsmeny.
  import { api } from "../../lib/api.js";
  import { logout } from "../../lib/auth.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import ThemeControls from "../../components/ThemeControls.svelte";
  import Foresporsler from "./Foresporsler.svelte";
  import Klienter from "./Klienter.svelte";

  let { firmId } = $props();

  let data = $state(null);

  async function load(id) {
    const [requests, clients, mine] = await Promise.all([
      api("/firms/" + id + "/requests"),
      api("/firms/" + id + "/clients"),
      api("/firms/mine"),
    ]);
    data = {
      firm: mine.firms.find((f) => f.firm_id === id) || null,
      pending: requests.requests.filter((r) => r.status === "pending"),
      clients: clients.clients,
    };
  }

  function reload() {
    load(firmId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    data = null;
    load(firmId).catch((error) => toast(error.message, false));
  });
</script>

<div class="navbar bg-base-100 shadow-sm">
  <div class="flex-1 gap-2 items-baseline">
    <a href="#/" class="btn btn-ghost text-xl">regnmed</a>
    <span class="text-sm opacity-70">{data?.firm?.name || ""}</span>
  </div>
  <div class="flex-none gap-2">
    <ThemeControls />
    <button class="btn btn-ghost btn-sm" onclick={logout}>Logg ut</button>
  </div>
</div>
<main class="p-6 max-w-4xl mx-auto">
  {#if !data}
    <span class="loading loading-spinner loading-lg"></span>
  {:else}
    <Foresporsler {firmId} pending={data.pending} onDone={reload} />
    <Klienter clients={data.clients} />
  {/if}
</main>
