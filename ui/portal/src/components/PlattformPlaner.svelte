<script>
  // Planer og priser (plattformkonsollen): pakkene og prislisten som nettsiden og
  // abonnementet leser. Alt er innsettings-bart — en endring er en ny versjon eller
  // en ny datert prisrad, aldri en overskriving. Support leser, systemadmin skriver.
  import { api, post, send } from "../lib/api.js";
  import { skjema } from "../lib/dialog.svelte.js";
  import { toast } from "../lib/toast.svelte.js";
  import { kr, today, formatDate } from "../lib/format.js";
  import { SLUG, planBody, priceBody, currentAndNext } from "../lib/planer.js";
  import Card from "./Card.svelte";

  let { systemadmin } = $props();

  let planer = $state(null);
  let nettside = $state({ plans: [], addons: [] });
  let historikk = $state(null); // slug med åpen prishistorikk
  const pakker = $derived((planer ?? []).filter((p) => p.kind !== "addon"));
  const tillegg = $derived((planer ?? []).filter((p) => p.kind === "addon"));

  async function last() {
    planer = null;
    try {
      const [a, b] = await Promise.all([api("/platform/plans"), api("/pricing")]);
      planer = a.plans;
      nettside = { plans: b.plans ?? [], addons: b.addons ?? [] };
    } catch (error) {
      planer = [];
      toast(error.message, false);
    }
  }

  $effect(() => {
    last();
  });

  const naa = today();
  const pris = (p) => (p ? kr(p.price_ore_per_month) + " kr" : "–");
  const grense = (n) => (n == null ? "uten grense" : new Intl.NumberFormat("nb-NO").format(n));

  // Spør til svaret er gyldig: en feil beholder det du skrev i stedet for å kaste det.
  async function spor(tittel, felter, bygg, valg) {
    let standard = {};
    for (;;) {
      const svar = await skjema(
        tittel,
        felter.map((f) => ({ ...f, standard: standard[f.navn] ?? f.standard })),
        valg,
      );
      if (!svar) return null;
      try {
        return bygg(svar);
      } catch (error) {
        toast(error.message, false);
        standard = svar;
      }
    }
  }

  const PLANFELT = [
    { navn: "name", etikett: "Navn" },
    { navn: "audience", etikett: "Passer for", valgfri: true },
    { navn: "voucher_limit", etikett: "Grense for bilag i året", valgfri: true, hjelp: "Tomt = uten grense. Grensene er myke og stopper aldri bokføring." },
    { navn: "employee_limit", etikett: "Grense for ansatte i lønn", valgfri: true, hjelp: "Tomt = uten grense." },
    { navn: "features", etikett: "Det som følger med (én per linje)", type: "textarea", valgfri: true },
    { navn: "sort", etikett: "Rekkefølge", valgfri: true, hjelp: "Lavest først. Tomt = 0." },
    { navn: "featured", etikett: "Fremhevet på nettsiden", type: "checkbox" },
    { navn: "listed", etikett: "Vises på nettsiden", type: "checkbox" },
  ];
  // En tilleggstjeneste kan vises som «kommer» før den finnes.
  const felterFor = (kind) =>
    kind === "addon"
      ? [...PLANFELT, { navn: "available", etikett: "Tilgjengelig", type: "checkbox", hjelp: "Av = vises som «kommer» og kan ikke kjøpes." }]
      : PLANFELT;

  async function lagrePlan(slug, kropp) {
    try {
      await send("PUT", "/platform/plans/" + slug, kropp);
      toast("Lagret som en ny versjon", true);
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Standardverdiene settes inn før dialogen åpnes.
  function medStandard(p) {
    return felterFor(p.kind).map((f) => ({
      ...f,
      standard:
        f.navn === "features" ? (p.features ?? []).join("\n")
        : f.navn === "voucher_limit" || f.navn === "employee_limit" ? (p[f.navn] ?? "")
        : f.navn === "sort" ? String(p.sort ?? 0)
        : p[f.navn] ?? f.standard ?? "",
    }));
  }

  async function rediger(p) {
    const kropp = await spor(
      "Rediger «" + p.slug + "»",
      medStandard(p),
      (v) => planBody({ ...v, kind: p.kind }),
      { ok: "Lagre", melding: "Lagres som en ny versjon. Tidligere versjoner blir stående i sporet." },
    );
    if (kropp) await lagrePlan(p.slug, kropp);
  }

  async function nyPlan() {
    const ny = await spor(
      "Ny pakke eller tilleggstjeneste",
      [
        { navn: "kind", etikett: "Hva er det", type: "select", standard: "plan", valg: [["plan", "pakke (plan)"], ["addon", "tilleggstjeneste"]] },
        { navn: "slug", etikett: "Kort navn i adressen", hjelp: "Små bokstaver, tall og bindestrek, for eksempel «liten» eller «bank». Kan ikke endres senere." },
      ],
      (v) => {
        const s = v.slug.trim();
        if (!SLUG.test(s)) throw new Error("bruk små bokstaver, tall og bindestrek");
        if (planer.some((p) => p.slug === s)) throw new Error("«" + s + "» finnes allerede");
        return { slug: s, kind: v.kind };
      },
      { ok: "Neste" },
    );
    if (!ny) return;
    const kropp = await spor(
      "Ny " + (ny.kind === "addon" ? "tilleggstjeneste" : "pakke") + " «" + ny.slug + "»",
      felterFor(ny.kind).map((f) => (f.navn === "listed" || f.navn === "available" ? { ...f, standard: false } : f)),
      (v) => planBody({ ...v, kind: ny.kind }),
      { ok: "Opprett", melding: "Vises ikke på nettsiden før du huker av for det, og har ingen pris før du legger inn en." },
    );
    if (kropp) await lagrePlan(ny.slug, kropp);
  }

  async function nyPris(p) {
    const kropp = await spor(
      "Ny pris for «" + p.slug + "»",
      [
        { navn: "interval", etikett: "Gjelder per", type: "select", standard: "month", valg: [["month", "måned"], ["year", "år (hele årsbeløpet)"]] },
        { navn: "price", etikett: "Pris i kroner, uten mva", hjelp: "For eksempel 129 eller 1 290,50. Bare hele øre." },
        { navn: "valid_from", etikett: "Gjelder fra", type: "date", standard: naa, hjelp: "I dag eller senere. Prisen kan aldri gjelde bakover." },
        { navn: "source", etikett: "Kilde", hjelp: "Vedtak eller sak prisen bygger på. Står i prissporet." },
      ],
      (v) => priceBody(v, naa),
      { ok: "Legg til", melding: "Prisen kan ikke endres eller slettes etterpå. En ny pris er en ny rad." },
    );
    if (!kropp) return;
    try {
      await post("/platform/plans/" + p.slug + "/prices", kropp);
      toast("Prisen er lagt til", true);
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#snippet tabell(rader, addon)}
  <table class="table table-sm">
    <thead>
      <tr>
        <th>{addon ? "Tjeneste" : "Pakke"}</th><th>Nettsiden</th>
        {#if addon}<th>Status</th>{:else}<th>Bilag/år</th><th>Ansatte</th>{/if}
        <th>Pris nå</th><th>Kommer</th><th></th>
      </tr>
    </thead>
    <tbody>
      {#each rader as p (p.slug)}
        {@const m = currentAndNext(p.prices, "month", naa)}
        {@const a = currentAndNext(p.prices, "year", naa)}
        <tr>
          <td>
            <div class="font-medium">{p.name}</div>
            <div class="text-xs opacity-60">{p.slug}</div>
          </td>
          <td>
            <span class="badge badge-sm {p.listed ? 'badge-success' : 'badge-ghost'}">{p.listed ? "vises" : "skjult"}</span>
            {#if p.featured}<span class="badge badge-sm badge-info">fremhevet</span>{/if}
          </td>
          {#if addon}
            <td><span class="badge badge-sm {p.available ? 'badge-success' : 'badge-warning'}">{p.available ? "tilgjengelig" : "kommer"}</span></td>
          {:else}
            <td>{grense(p.voucher_limit)}</td>
            <td>{grense(p.employee_limit)}</td>
          {/if}
          <td>
            {pris(m.current)} <span class="opacity-60">/mnd</span>
            {#if a.current}<div class="text-xs opacity-70">{pris(a.current)} /år</div>{/if}
          </td>
          <td>
            {#if m.next}{pris(m.next)} fra {formatDate(m.next.valid_from)}{:else}<span class="opacity-50">–</span>{/if}
            {#if a.next}<div class="text-xs opacity-70">{pris(a.next)} /år fra {formatDate(a.next.valid_from)}</div>{/if}
          </td>
          <td class="whitespace-nowrap text-right">
            <button
              class="btn btn-ghost btn-xs"
              aria-expanded={historikk === p.slug}
              onclick={() => (historikk = historikk === p.slug ? null : p.slug)}
            >
              Prishistorikk
            </button>
            {#if systemadmin}
              <button class="btn btn-ghost btn-xs" onclick={() => rediger(p)}>Rediger</button>
              <button class="btn btn-ghost btn-xs" onclick={() => nyPris(p)}>Ny pris</button>
            {/if}
          </td>
        </tr>
        {#if historikk === p.slug}
          <tr>
            <td colspan={addon ? 6 : 7} class="bg-base-200">
              <table class="table table-xs">
                <caption class="text-left text-xs opacity-70 mb-1">Prishistorikk for {p.name}, nyeste først</caption>
                <thead><tr><th>Per</th><th>Pris</th><th>Gjelder fra</th><th>Kilde</th></tr></thead>
                <tbody>
                  {#each p.prices as r, i (i)}
                    <tr>
                      <td>{r.interval === "year" ? "år" : "måned"}</td>
                      <td>{pris(r)}</td>
                      <td>{formatDate(r.valid_from)}{r.valid_from > naa ? " (kommer)" : ""}</td>
                      <td>{r.source}</td>
                    </tr>
                  {:else}
                    <tr><td colspan="4" class="opacity-70">Ingen priser ennå. Vises uten pris.</td></tr>
                  {/each}
                </tbody>
              </table>
              {#if p.features?.length}
                <p class="text-xs opacity-70 mt-2">Følger med: {p.features.join(" · ")}</p>
              {/if}
              <p class="text-xs opacity-60 mt-1">Sist endret av {p.set_by} {formatDate(p.created_at)}.</p>
            </td>
          </tr>
        {/if}
      {:else}
        <tr><td colspan={addon ? 6 : 7} class="opacity-70">{addon ? "Ingen tilleggstjenester ennå." : "Ingen pakker ennå."}</td></tr>
      {/each}
    </tbody>
  </table>
{/snippet}

{#snippet visning(p, addon)}
  <div class="rounded-box border border-base-300 p-3 {p.featured ? 'bg-base-200 border-primary' : ''}">
    <div class="font-semibold">{p.name}</div>
    <div class="text-2xl font-bold">{p.price_ore_per_month == null ? "pris kommer" : pris(p)}</div>
    <div class="text-xs mb-1">
      {#if addon}
        <span class="badge badge-xs {p.available ? 'badge-success' : 'badge-warning'}">{p.available ? "tilgjengelig" : "kommer"}</span>
      {:else}
        <span class="badge badge-xs {p.in_force ? 'badge-success' : 'badge-warning'}">{p.in_force ? "i kraft" : "ikke i kraft ennå"}</span>
      {/if}
      {#if p.upcoming}<span class="opacity-70">{pris(p.upcoming)} fra {formatDate(p.upcoming.valid_from)}</span>{/if}
    </div>
    <div class="text-xs opacity-70">
      {p.audience}
      {#if !addon}<br />Bilag: {grense(p.voucher_limit)} · Ansatte: {grense(p.employee_limit)}{/if}
    </div>
    {#if p.features?.length}
      <ul class="text-xs list-disc ml-4 mt-1">
        {#each p.features as f, i (i)}<li>{f}</li>{/each}
      </ul>
    {/if}
  </div>
{/snippet}

{#if !planer}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <Card title="Pakker">
    <p class="text-sm opacity-70 mb-3">
      Pakkene og prisene som nettsiden viser og som abonnementet bygger på. Endringer er
      innsettings-bare og loggføres.
      {#if !systemadmin}Du har lesetilgang. Endringer krever systemadmin.{/if}
    </p>
    {@render tabell(pakker, false)}
    {#if systemadmin}
      <div class="mt-3"><button class="btn btn-sm btn-primary" onclick={nyPlan}>Ny pakke eller tilleggstjeneste</button></div>
    {/if}
  </Card>

  <Card title="Tilleggstjenester">
    <p class="text-sm opacity-70 mb-3">
      Kjøpes i tillegg til en pakke. En tjeneste som ikke er tilgjengelig vises som «kommer» og kan ikke kjøpes.
    </p>
    {@render tabell(tillegg, true)}
  </Card>

  <Card title="Slik ser prisene ut på nettsiden">
    <p class="text-sm opacity-70 mb-3">
      Det den åpne adressen <code>/pricing</code> sender akkurat nå, bare det som vises.
      Priser er per selskap per måned, uten mva.
    </p>
    <div class="grid gap-3 sm:grid-cols-3">
      {#each nettside.plans as p (p.slug)}{@render visning(p, false)}
      {:else}<span class="opacity-70 text-sm">Ingen pakker vises på nettsiden.</span>{/each}
    </div>
    {#if nettside.addons.length}
      <h3 class="font-semibold mt-4 mb-2">Tilleggstjenester</h3>
      <div class="grid gap-3 sm:grid-cols-3">
        {#each nettside.addons as p (p.slug)}{@render visning(p, true)}{/each}
      </div>
    {/if}
  </Card>
{/if}
