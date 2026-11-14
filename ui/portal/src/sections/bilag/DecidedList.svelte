<script>
  // Behandlede dokumenter: bokført eller avvist (med grunnen).
  // Historikken bare vokser, så den hentes i sider.
  import { api } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";

  let { companyId } = $props();

  let limit = $state(50);
  let docs = $state(null);
  let error = $state(null);

  $effect(() => {
    api("/companies/" + companyId + "/inbox?status=behandlet&limit=" + limit)
      .then((r) => (docs = r.documents))
      .catch((e) => (error = e.message));
  });
</script>

<div class="card border border-base-300 bg-base-100 mb-6">
  <div class="card-body overflow-x-auto">
    <h2 class="card-title">Behandlede dokumenter</h2>
    {#if error}
      <div class="alert alert-error" role="alert"><span>{error}</span></div>
    {:else if docs === null}
      <span class="loading loading-spinner"></span>
    {:else if docs.length === 0}
      <p class="opacity-70">Ingen dokumenter er behandlet ennå.</p>
    {:else}
      <table class="table">
        <thead>
          <tr>
            <th scope="col">Dokument</th>
            <th scope="col">Utfall</th>
            <th scope="col" class="hidden sm:table-cell">Av</th>
          </tr>
        </thead>
        <tbody>
          {#each docs as d (d.document_id)}
            <tr>
              <td class="break-all">
                {d.filename}
                <span class="block text-sm opacity-70">{formatDate(d.uploaded_at)}</span>
              </td>
              <td>
                {#if d.status === "bokfort"}
                  <span class="badge badge-success badge-sm">bokført</span>
                {:else if d.status === "utlegg"}
                  <a class="badge badge-info badge-soft badge-sm" href={"#/c/" + companyId + "/utlegg/" + d.expense_id}>utleggskrav</a>
                {:else}
                  <span class="badge badge-ghost badge-sm">avvist</span>
                  {#if d.note}<span class="block text-sm opacity-70">{d.note}</span>{/if}
                {/if}
              </td>
              <td class="hidden sm:table-cell">{d.decided_by || "–"}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if docs.length >= limit && limit < 200}
        <button class="btn btn-sm mt-3 w-fit" onclick={() => (limit = Math.min(limit + 50, 200))}>Vis flere</button>
      {/if}
    {/if}
  </div>
</div>
