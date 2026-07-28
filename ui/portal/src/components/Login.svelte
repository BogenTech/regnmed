<script>
  import { login } from "../lib/auth.svelte.js";
  import ThemeControls from "./ThemeControls.svelte";
</script>

<div class="min-h-screen flex items-center justify-center">
  <div class="card bg-base-100 shadow-sm w-full max-w-sm">
    <div class="card-body items-center">
      <h1 class="card-title text-2xl">regnmed</h1>
      <p class="opacity-70 text-sm mb-2">Regnskap du kan etterprøve.</p>
      <button class="btn btn-primary w-full" onclick={login}>Logg inn</button>
      <div class="mt-2 flex gap-2 items-center"><ThemeControls /></div>
    </div>
  </div>
</div>
