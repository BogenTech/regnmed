<script>
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import Card from "../../components/Card.svelte";

  let { companyId, vouchers } = $props();

  let attachments = $state(null); // liste for det bilaget som er «vist»

  async function lastOppVedlegg(voucher, event) {
    const file = event.target.files[0];
    if (!file) return;
    event.target.value = "";
    try {
      const uploaded = await api(
        "/companies/" +
          companyId +
          "/vouchers/" +
          voucher.voucher_id +
          "/attachments?filename=" +
          encodeURIComponent(file.name),
        {
          method: "POST",
          headers: { "content-type": file.type || "application/octet-stream" },
          body: file,
        },
      );
      toast("Vedlegg lagret (sha256 " + uploaded.sha256.slice(0, 12) + "…)", true);
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function vis(voucher) {
    try {
      const listing = await api(
        "/companies/" + companyId + "/vouchers/" + voucher.voucher_id + "/attachments",
      );
      attachments = listing.attachments;
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Bilag">
  <table class="table table-sm">
    <thead>
      <tr><th>Bilag</th><th>Dato</th><th>Tekst</th><th></th></tr>
    </thead>
    <tbody>
      {#each vouchers as v (v.voucher_id)}
        <tr>
          <td>{v.voucher}</td>
          <td>{v.date}</td>
          <td>{v.description}</td>
          <td>
            <label class="btn btn-xs btn-outline">
              Vedlegg
              <input type="file" class="hidden" onchange={(e) => lastOppVedlegg(v, e)} />
            </label>
            <button class="btn btn-xs btn-ghost" onclick={() => vis(v)}>Vis</button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if attachments}
    <div class="mt-4">
      {#if attachments.length}
        {#each attachments as a (a.attachment_id)}
          <div class="flex gap-2 items-center text-sm py-1">
            <button
              class="link"
              onclick={() =>
                download("/companies/" + companyId + "/attachments/" + a.attachment_id, a.filename)}
            >
              {a.filename}
            </button>
            <span class="opacity-60">
              {a.byte_size} B · sha256 <span class="font-mono">{a.sha256.slice(0, 16)}…</span> ·
              {a.uploaded_by}
            </span>
          </div>
        {/each}
      {:else}
        <p class="opacity-70 text-sm">Ingen vedlegg på bilaget.</p>
      {/if}
    </div>
  {/if}
</Card>
