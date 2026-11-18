<script>
  // Abonnementsbanneret (#65, docs/abonnement.md). Vises i skallet, så
  // det følger alle seksjoner. Portalen varsler — SERVEREN sperrer, i
  // tilgangsvakten; banneret er bare beskjeden om hvorfor.
  import { api } from "../lib/api.js";
  import { formatDate } from "../lib/format.js";

  let { company } = $props();
  let a = $derived(company?.abonnement);

  // Over pakkens bilagsgrense: bare en rolig beskjed, bokføringen stoppes aldri.
  // The subscription read is cached by api.js, so section switches cost nothing.
  let overBilag = $state(false);
  $effect(() => {
    const id = company?.company_id;
    overBilag = false;
    if (!id) return;
    api("/companies/" + id + "/subscription")
      .then((s) => {
        if (company?.company_id === id) overBilag = !!s?.bruk?.over_bilag;
      })
      .catch(() => {}); // no read right, or offline: the banner just stays quiet
  });
</script>

{#if overBilag}
  <div class="alert alert-info mb-4 text-sm" role="status">
    <span>
      Selskapet har flere bilag i år enn pakken er laget for. Alt virker som før.
      <a class="link" href={"#/c/" + company.company_id + "/admin/abonnement"}>Se pakkene</a>
    </span>
  </div>
{/if}

{#if a && a.status !== "aktiv"}
  {#if a.status === "prove"}
    <div class="alert alert-info mb-4 text-sm">
      <span>Prøvetid til {formatDate(a.dato) || ""}. Alt virker som normalt.</span>
    </div>
  {:else if a.status === "frist"}
    <div class="alert alert-warning mb-4 text-sm">
      <span>
        Abonnementet er ikke i orden. Alt virker fram til {formatDate(a.dato) || ""} — etter det
        sperres endringer, mens lesing og eksport alltid virker.
      </span>
    </div>
  {:else}
    <div class="alert alert-error mb-4 text-sm">
      <span>
        Abonnementet er utløpt: endringer er sperret{a.dato ? " siden " + formatDate(a.dato) : ""}.
        Lesing og eksport virker som før — hovedboken tas aldri som gissel.
      </span>
    </div>
  {/if}
{/if}
