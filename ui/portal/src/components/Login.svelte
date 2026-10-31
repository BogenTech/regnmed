<script>
  import { login } from "../lib/auth.svelte.js";
  import ThemeControls from "./ThemeControls.svelte";
  import DeviceLogin from "./DeviceLogin.svelte";

  let enhet = $state(false);
</script>

<div class="hero min-h-screen">
  <div class="hero-content card border border-base-300 bg-base-100 w-full max-w-sm">
    <div class="card-body items-center">
      <h1 class="card-title text-2xl">regnmed</h1>
      <p class="opacity-70 text-sm mb-2">Regnskap du kan etterprøve.</p>
      {#if enhet}
        <DeviceLogin onavbryt={() => (enhet = false)} />
      {:else}
        <button class="btn btn-primary w-full" onclick={login}>Logg inn</button>
        <button class="btn btn-ghost btn-sm w-full" onclick={() => (enhet = true)}>
          Logg inn med telefonen
        </button>
        <div class="mt-2 flex gap-2 items-center"><ThemeControls /></div>
      {/if}
    </div>
  </div>
</div>
