<script>
  import { toasts, closeToast } from "../lib/toast.svelte.js";
</script>

<!-- Always in the page, so a screen reader is already listening when a
     message arrives: confirmations politely through the container, errors
     (role=alert) at once. A second live region per item would announce
     confirmations twice. -->
<div class="toast toast-top toast-end z-50" aria-live="polite">
  {#each toasts as t (t.id)}
    <div role={t.ok ? undefined : "alert"} class="alert {t.ok ? 'alert-success' : 'alert-error'} shadow max-w-sm">
      <span class="whitespace-normal">{t.message}</span>
      <button class="btn btn-ghost btn-xs btn-circle" aria-label="Lukk meldingen" onclick={() => closeToast(t.id)}>✕</button>
    </div>
  {/each}
</div>
