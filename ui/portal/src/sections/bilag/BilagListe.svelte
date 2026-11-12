<script>
  // Bokførte bilag, søkbare og i sider (GET /vouchers?sok=&limit=), med
  // vedleggene — og muligheten til å legge et vedlegg på et bilag som
  // allerede er bokført.
  import { api } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";

  let { companyId } = $props();

  const PAGE = 50;
  let search = $state("");
  let limit = $state(PAGE);
  let rows = $state(null);
  let total = $state(0);
  let open = $state(null); // { voucherId, attachments }

  let seq = 0;
  let timer;
  $effect(() => {
    const q = search.trim();
    const n = limit;
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const mine = ++seq;
      try {
        const r = await api(
          "/companies/" + companyId + "/vouchers?limit=" + n + (q ? "&sok=" + encodeURIComponent(q) : ""),
        );
        if (mine === seq) {
          rows = r.vouchers;
          total = r.total;
        }
      } catch (e) {
        toast(e.message, false);
      }
    }, q ? 250 : 0);
    return () => clearTimeout(timer);
  });

  async function attach(v, event) {
    const file = event.target.files[0];
    if (!file) return;
    event.target.value = "";
    try {
      await api(
        "/companies/" + companyId + "/vouchers/" + v.voucher_id + "/attachments?filename=" + encodeURIComponent(file.name),
        { method: "POST", headers: { "content-type": file.type || "application/octet-stream" }, body: file },
      );
      toast("Vedlegget er lagt på bilag " + v.voucher, true);
      if (open?.voucherId === v.voucher_id) show(v);
    } catch (e) {
      toast(e.message, false);
    }
  }

  async function show(v) {
    if (open?.voucherId === v.voucher_id) return (open = null);
    try {
      const r = await api("/companies/" + companyId + "/vouchers/" + v.voucher_id + "/attachments");
      open = { voucherId: v.voucher_id, attachments: r.attachments };
    } catch (e) {
      toast(e.message, false);
    }
  }
</script>

<div class="card border border-base-300 bg-base-100 mb-6">
  <div class="card-body overflow-x-auto">
    <h2 class="card-title">Bokførte bilag</h2>
    <label class="input w-full sm:w-80">
      <span class="sr-only">Søk i bilag</span>
      <input type="search" placeholder="Bilagsnr, tekst, dato eller konto" bind:value={search} />
    </label>
    {#if rows === null}
      <span class="loading loading-spinner"></span>
    {:else if rows.length === 0}
      <p class="opacity-70">{search.trim() ? "Ingen bilag passer søket." : "Ingen bilag ennå."}</p>
    {:else}
      <table class="table">
        <thead>
          <tr>
            <th scope="col">Bilag</th>
            <th scope="col" class="hidden sm:table-cell">Dato</th>
            <th scope="col"><span class="sr-only">Vedlegg</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rows as v (v.voucher_id)}
            <tr>
              <td>
                {v.voucher}
                <span class="block text-sm opacity-70">{v.description}<span class="sm:hidden">, {formatDate(v.date)}</span></span>
              </td>
              <td class="hidden sm:table-cell whitespace-nowrap">{formatDate(v.date)}</td>
              <td class="whitespace-nowrap text-right">
                <button class="btn btn-ghost btn-xs" onclick={() => show(v)} aria-expanded={open?.voucherId === v.voucher_id}>
                  Vedlegg
                </button>
                <label class="btn btn-ghost btn-xs">
                  Legg til
                  <input type="file" class="sr-only" aria-label={"Legg vedlegg på bilag " + v.voucher} onchange={(e) => attach(v, e)} />
                </label>
              </td>
            </tr>
            {#if open?.voucherId === v.voucher_id}
              <tr>
                <td colspan="3">
                  {#each open.attachments as a (a.attachment_id)}
                    <button
                      class="link block text-sm"
                      onclick={() => download("/companies/" + companyId + "/attachments/" + a.attachment_id, a.filename)}
                    >
                      {a.filename}
                    </button>
                  {:else}
                    <span class="text-sm opacity-70">Ingen vedlegg på bilaget.</span>
                  {/each}
                </td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
      {#if rows.length < total}
        <button class="btn btn-sm mt-3 w-fit" onclick={() => (limit = Math.min(limit + PAGE, 500))}>
          Vis flere ({rows.length} av {total})
        </button>
      {/if}
    {/if}
  </div>
</div>
