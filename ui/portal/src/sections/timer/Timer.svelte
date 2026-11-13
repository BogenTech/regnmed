<script>
  // Timeføring (#38): uken kommer fra adressen (?uke=), så ukeknappene er
  // vanlige lenker og uken kan deles.
  import { api } from "../../lib/api.js";
  import { today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import LoadError from "../../components/LoadError.svelte";
  import MinUke from "./MinUke.svelte";
  import PerProsjekt from "./PerProsjekt.svelte";
  import { addDays, mondayOf, weekTitle } from "./week.js";

  let { companyId } = $props();

  function ukeFraHash() {
    const params = new URLSearchParams(location.hash.split("?")[1] || "");
    const uke = params.get("uke");
    return mondayOf(/^\d{4}-\d{2}-\d{2}$/.test(uke || "") ? uke : today());
  }

  let mandag = $state(ukeFraHash());

  // Ruteren fanger bare stien, ikke spørrestrengen — uken lyttes ut selv.
  $effect(() => {
    const oppdater = () => (mandag = ukeFraHash());
    window.addEventListener("hashchange", oppdater);
    return () => window.removeEventListener("hashchange", oppdater);
  });

  let denneUken = $derived(mondayOf(today()));
  let uke = $derived({
    from: mandag,
    to: addDays(mandag, 6),
    forrige: addDays(mandag, -7),
    neste: addDays(mandag, 7),
  });

  // The page keeps showing the week it has (dimmed) while the next one
  // loads, and only the NEWEST load may write `data`: an older answer
  // landing late would otherwise put one week's hours under another
  // week's heading. A superseded load resolves when the newer one does,
  // so a caller awaiting reload() always continues on current data.
  let data = $state(null);
  let seq = 0;
  let latest = Promise.resolve();

  function sheet(id, from) {
    const to = addDays(from, 6);
    return Promise.all([
      api("/companies/" + id + "/timesheet?from=" + from + "&to=" + to),
      api("/companies/" + id + "/timesheet/summary?from=" + from + "&to=" + to)
        .then((s) => s.prosjekter)
        .catch(() => null),
    ]);
  }

  function load(id, from, keep) {
    const mine = ++seq;
    const run = (async () => {
      // Last week fills empty rows and «Kopier forrige uke»; the project
      // register gives names and defaults. Neither changes when a cell is
      // saved, so a reload after a save (`keep`) fetches only this week.
      const [[timesheet, summary], forrigeUke, dims] = await Promise.all([
        sheet(id, from),
        keep
          ? keep.forrigeUke
          : api("/companies/" + id + "/timesheet?from=" + addDays(from, -7) + "&to=" + addDays(from, -1))
              .catch(() => ({ entries: [] })),
        keep ? keep.dims : api("/companies/" + id + "/dimensions").then((d) => d.dimensions, () => []),
      ]);
      if (mine !== seq) return latest;
      data = { from, uke: timesheet, forrigeUke, summary, dims };
    })();
    latest = run;
    return run;
  }

  function reload() {
    const keep = data && data.from === uke.from ? data : null;
    return load(companyId, uke.from, keep).catch((error) => toast(error.message, false));
  }

  // A week that failed to load says so with a retry, instead of leaving
  // the previous week dimmed (or a spinner) for ever.
  let feil = $state(null);
  function loadWeek() {
    feil = null;
    const from = uke.from;
    load(companyId, from, null).catch((error) => {
      if (from === uke.from) feil = error.message;
    });
  }
  $effect(loadWeek);

  let laster = $derived(!feil && (!data || data.from !== uke.from));
  const lenke = (mandag) => "#/c/" + companyId + "/timer?uke=" + mandag;
</script>

<div class="flex flex-wrap items-center gap-2 mb-3">
  <h2 class="text-lg font-semibold mr-auto">{weekTitle(uke.from)}</h2>
  <nav class="join" aria-label="Velg uke">
    <a class="btn btn-sm join-item" href={lenke(uke.forrige)}>‹ Forrige uke</a>
    {#if uke.from !== denneUken}
      <a class="btn btn-sm join-item" href={lenke(denneUken)}>Denne uken</a>
    {/if}
    <a class="btn btn-sm join-item" href={lenke(uke.neste)}>Neste uke ›</a>
  </nav>
</div>

{#if feil}
  <LoadError error={feil} retry={loadWeek} />
{:else if !data}
  <span class="loading loading-spinner loading-lg" aria-label="Laster timer"></span>
{:else}
  <div class={laster ? "opacity-50 pointer-events-none" : ""} aria-busy={laster}>
    {#key data.from}
      <MinUke
        {companyId}
        uke={data.uke}
        forrigeUke={data.forrigeUke}
        from={data.from}
        to={addDays(data.from, 6)}
        dims={data.dims}
        onDone={reload}
      />
    {/key}

    {#if data.summary}
      <PerProsjekt {companyId} summary={data.summary} dims={data.dims} />
      <p class="text-xs opacity-60 -mt-4 mb-6 ml-1">
        Fakturering av timene skjer under
        <a class="link" href={"#/c/" + companyId + "/faktura"}>Faktura</a>.
      </p>
    {/if}
  </div>
{/if}
