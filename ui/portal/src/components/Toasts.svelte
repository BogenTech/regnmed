<script>
  import { toasts } from "../lib/toast.svelte.js";
</script>

{#if toasts.length}
  <div class="toast toast-top toast-end z-50">
    {#each toasts as t (t.id)}
      <div class="alert {t.ok ? 'alert-success' : 'alert-error'} shadow">
        <span>{t.message}</span>
      </div>
    {/each}
  </div>
{/if}
