<script>
  // Timeføring (#38): uken kommer fra adressen (?uke=), så « og » er
  // vanlige lenker og uken kan deles.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import MinUke from "./MinUke.svelte";
  import PerProsjekt from "./PerProsjekt.svelte";

  let { companyId } = $props();

  function ukeFraHash() {
    const params = new URLSearchParams(location.hash.split("?")[1] || "");
    const uke = params.get("uke");
    if (uke) return uke;
    const now = new Date();
    const day = (now.getDay() + 6) % 7;
    now.setDate(now.getDate() - day);
    return now.toISOString().slice(0, 10);
  }

  let mandag = $state(ukeFraHash());

  // Ruteren fanger bare stien, ikke spørrestrengen — uken lyttes ut selv.
  $effect(() => {
    const oppdater = () => (mandag = ukeFraHash());
    window.addEventListener("hashchange", oppdater);
    return () => window.removeEventListener("hashchange", oppdater);
  });

  let uke = $derived.by(() => {
    const start = new Date(mandag + "T00:00:00Z");
    const slutt = new Date(start);
    slutt.setUTCDate(slutt.getUTCDate() + 6);
    const forrige = new Date(start);
    forrige.setUTCDate(forrige.getUTCDate() - 7);
    const forrigeSlutt = new Date(start);
    forrigeSlutt.setUTCDate(forrigeSlutt.getUTCDate() - 1);
    const neste = new Date(start);
    neste.setUTCDate(neste.getUTCDate() + 7);
    return {
      from: start.toISOString().slice(0, 10),
      to: slutt.toISOString().slice(0, 10),
      forrige: forrige.toISOString().slice(0, 10),
      forrigeSlutt: forrigeSlutt.toISOString().slice(0, 10),
      neste: neste.toISOString().slice(0, 10),
    };
  });

  let data = $state(null);

  async function load(id, from, to, forrigeFra, forrigeTil) {
    // Forrige uke hentes med: gridet lager tomme rader av forrige ukes
    // prosjekter, og «Kopier forrige uke» trenger linjene. Fakturering
    // av timene bor under Faktura (grunnlaget hentes der).
    const [timesheet, forrigeUke, summary, dims] = await Promise.all([
      api("/companies/" + id + "/timesheet?from=" + from + "&to=" + to),
      api("/companies/" + id + "/timesheet?from=" + forrigeFra + "&to=" + forrigeTil).catch(
        () => ({ entries: [] }),
      ),
      api("/companies/" + id + "/timesheet/summary?from=" + from + "&to=" + to).catch(() => null),
      api("/companies/" + id + "/dimensions").catch(() => ({ dimensions: [] })),
    ]);
    data = {
      uke: timesheet,
      forrigeUke,
      summary: summary ? summary.prosjekter : null,
      dims: dims.dimensions,
    };
  }

  function reload() {
    load(companyId, uke.from, uke.to, uke.forrige, uke.forrigeSlutt).catch((error) =>
      toast(error.message, false),
    );
  }

  $effect(() => {
    const id = companyId;
    const { from, to, forrige, forrigeSlutt } = uke;
    data = null;
    load(id, from, to, forrige, forrigeSlutt).catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <MinUke
    {companyId}
    uke={data.uke}
    forrigeUke={data.forrigeUke}
    from={uke.from}
    to={uke.to}
    forrige={uke.forrige}
    neste={uke.neste}
    dims={data.dims}
    onDone={reload}
  />

  {#if data.summary}
    <PerProsjekt {companyId} summary={data.summary} />
    <p class="text-xs opacity-60 -mt-4 mb-6 ml-1">
      Fakturering av timene skjer under
      <a class="link" href={"#/c/" + companyId + "/faktura"}>Faktura</a>.
    </p>
  {/if}
{/if}
