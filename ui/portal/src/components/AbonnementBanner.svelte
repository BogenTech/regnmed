<script>
  // Abonnementsbanneret (#65, docs/abonnement.md). Vises i skallet, så
  // det følger alle seksjoner. Portalen varsler — SERVEREN sperrer, i
  // tilgangsvakten; banneret er bare beskjeden om hvorfor.
  let { company } = $props();
  let a = $derived(company?.abonnement);
</script>

{#if a && a.status !== "aktiv"}
  {#if a.status === "prove"}
    <div class="alert alert-info mb-4 text-sm">
      <span>Prøvetid til {a.dato || ""}. Alt virker som normalt.</span>
    </div>
  {:else if a.status === "frist"}
    <div class="alert alert-warning mb-4 text-sm">
      <span>
        Abonnementet er ikke i orden. Alt virker fram til {a.dato || ""} — etter det
        sperres endringer, mens lesing og eksport alltid virker.
      </span>
    </div>
  {:else}
    <div class="alert alert-error mb-4 text-sm">
      <span>
        Abonnementet er utløpt: endringer er sperret{a.dato ? " siden " + a.dato : ""}.
        Lesing og eksport virker som før — hovedboken tas aldri som gissel.
      </span>
    </div>
  {/if}
{/if}
