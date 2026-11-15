<script>
  // Varelager (#39): beholdning og verdi er ALLTID beregnet fra de
  // innsettings-bare bevegelsene — ingenting lagres som saldo.
  //
  // The stock count posts a voucher, so it is two steps: the server
  // computes the count with ?preview=true (nothing committed), the
  // person sees every difference and the amount the voucher will move,
  // and only then posts it.
  import { tick } from "svelte";
  import { post } from "../../lib/api.js";
  import { kr, today, antallStr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import Bevegelser from "./Bevegelser.svelte";
  import { lesAntall, lesPris, tellingLinjer } from "./produkt.js";

  let { companyId, products, inventory, verdiOre, kanSkrive, onChanged } = $props();

  let lagerforte = $derived(products.filter((p) => p.lagerfort && p.aktiv));
  let apen = $state(null);

  // --- Varekjøp / justering ---------------------------------------------
  const tomBevegelse = () => ({ produkt: "", kind: "kjop", dato: today(), antall: "", kost: "", note: "" });
  let ny = $state(tomBevegelse());
  let produkt = $derived(ny.produkt || lagerforte[0]?.nummer || "");
  let registrerer = $state(false);

  async function registrer(event) {
    event.preventDefault();
    if (registrerer) return;
    const antall = lesAntall(ny.antall);
    if (antall.feil) return toast(antall.feil, false);
    const body = { produkt, dato: ny.dato, kind: ny.kind, antall_milli: antall.milli, note: ny.note.trim() || null };
    if (ny.kind === "kjop") {
      if (antall.milli < 0)
        return toast("Et varekjøp må ha positivt antall — en nedjustering registreres som justering", false);
      const kost = lesPris(ny.kost, "kostprisen");
      if (kost.feil) return toast(kost.feil, false);
      body.kostpris_ore = kost.ore;
    } else if (!body.note) {
      return toast("En justering må ha et notat som sier hvorfor", false);
    }
    registrerer = true;
    try {
      await post("/companies/" + companyId + "/inventory/movements", body);
      toast((ny.kind === "kjop" ? "Varekjøpet" : "Justeringen") + " er registrert", true);
      ny = { ...tomBevegelse(), produkt: ny.produkt, kind: ny.kind };
      await onChanged();
    } catch (error) {
      toast(error.message, false);
    } finally {
      registrerer = false;
    }
  }

  // --- Varetelling --------------------------------------------------------
  let teller = $state(false);
  let talt = $state({});
  let tellingDato = $state(today());
  let forhandsvisning = $state(null);
  let bokforer = $state(false);

  // Focus follows each step, so the keyboard never drops to the top.
  const fokus = async (id) => (await tick(), document.getElementById(id)?.focus());

  function startTelling() {
    teller = true;
    fokus("talt-" + inventory[0]?.nummer);
  }

  function endreTelling() {
    forhandsvisning = null;
    fokus("talt-" + inventory[0]?.nummer);
  }

  function avbrytTelling() {
    teller = false;
    talt = {};
    forhandsvisning = null;
    fokus("start-telling");
  }

  function telling() {
    const t = tellingLinjer(inventory, talt);
    if (t.feil) {
      toast(t.feil, false);
      return null;
    }
    return { dato: tellingDato, linjer: t.linjer };
  }

  async function seOver(event) {
    event.preventDefault();
    const body = telling();
    if (!body) return;
    try {
      const svar = await post("/companies/" + companyId + "/inventory/count?preview=true", body);
      forhandsvisning = { body, svar };
      fokus("telling-oversikt");
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function bokfor() {
    if (bokforer || !forhandsvisning) return;
    bokforer = true;
    try {
      const svar = await post("/companies/" + companyId + "/inventory/count", {
        ...forhandsvisning.body,
        forventet_endring_ore: forhandsvisning.svar.endring_ore,
      });
      toast(
        "Tellingen er registrert" + (svar.voucher ? " og bokført som bilag " + svar.voucher : ", uten verdiendring"),
        true,
      );
      avbrytTelling();
      tellingDato = today();
      await onChanged();
    } catch (error) {
      toast(error.message, false);
    } finally {
      bokforer = false;
    }
  }

  const navn = (nummer) => inventory.find((r) => r.nummer === nummer)?.navn ?? "";
  let endringer = $derived(
    forhandsvisning?.svar.justeringer.filter((j) => j.talt_milli !== j.bokfort_milli) ?? [],
  );

  const detaljer = (r) =>
    [
      antallStr(r.antall_milli) + " stk",
      r.gjennomsnitt_ore == null ? null : "snitt " + kr(r.gjennomsnitt_ore) + " kr",
      "verdi " + kr(r.verdi_ore) + " kr",
    ]
      .filter(Boolean)
      .join(", ");
</script>

<Card title="Varelager">
  <p class="text-sm opacity-70 mb-2">
    Beholdning og verdi regnes alltid ut fra bevegelsene, med gjennomsnittlig kostpris. Salg trekkes
    fra når fakturaen utstedes, og en kreditnota legger varene tilbake.
  </p>

  <ul class="divide-y divide-base-200">
    {#each inventory as r (r.nummer)}
      <li class="py-2">
        <div class="flex items-center gap-2">
          <div class="min-w-0 flex-1">
            <div class="truncate"><span class="font-medium">{r.nummer}</span> {r.navn}</div>
            <div class="text-xs opacity-60" class:text-error={r.antall_milli < 0}>{detaljer(r)}</div>
          </div>
          {#if teller}
            <label class="fieldset py-0 shrink-0">
              <span class="sr-only">Talt antall {r.nummer} {r.navn}</span>
              <input
                id={"talt-" + r.nummer}
                form="telling"
                class="input input-sm w-20"
                inputmode="decimal"
                placeholder="Talt"
                bind:value={talt[r.nummer]}
                oninput={() => (forhandsvisning = null)}
              />
            </label>
          {:else}
            <button
              class="btn btn-xs btn-ghost shrink-0"
              aria-expanded={apen === r.nummer}
              aria-label={(apen === r.nummer ? "Skjul bevegelser for " : "Bevegelser for ") + r.nummer}
              onclick={() => (apen = apen === r.nummer ? null : r.nummer)}
            >
              {apen === r.nummer ? "Skjul" : "Bevegelser"}
            </button>
          {/if}
        </div>
        {#if apen === r.nummer && !teller}
          <Bevegelser {companyId} nummer={r.nummer} />
        {/if}
      </li>
    {/each}
  </ul>
  <p class="text-sm mt-2">Samlet verdi: <span class="font-medium">{kr(verdiOre)} kr</span></p>

  {#if kanSkrive}
    {#if teller}
      <!-- The "Talt" inputs in the list above belong to this form, so Enter
           in any of them submits the count for review. -->
      <form id="telling" class="flex flex-wrap gap-x-3 gap-y-1 items-end mt-3" onsubmit={seOver}>
        <label class="fieldset py-0">
          <span class="fieldset-legend">Tellingsdato</span>
          <input
            type="date"
            class="input input-sm"
            required
            bind:value={tellingDato}
            oninput={() => (forhandsvisning = null)}
          />
        </label>
        <div class="flex gap-2 py-1">
          <button class="btn btn-sm">Se over tellingen</button>
          <button type="button" class="btn btn-sm btn-ghost" onclick={avbrytTelling}>Avbryt</button>
        </div>
      </form>
      <p class="text-xs opacity-60 mt-1">
        Fyll inn det du har talt. Produkter du lar stå tomme, blir ikke endret.
      </p>
      {#if forhandsvisning}
        {@const s = forhandsvisning.svar}
        <div
          id="telling-oversikt"
          tabindex="-1"
          role="region"
          aria-label="Oversikt over tellingen"
          class="mt-3 p-3 rounded-box bg-base-200 text-sm"
        >
          <h3 class="font-semibold mb-1">Tellingen {formatDate(forhandsvisning.body.dato)}</h3>
          {#if endringer.length}
            <ul class="mb-2">
              {#each endringer as j (j.produkt)}
                <li class="flex gap-2">
                  <span class="min-w-0 flex-1 truncate">{j.produkt} {navn(j.produkt)}</span>
                  <span class="whitespace-nowrap">
                    {antallStr(j.bokfort_milli)} → {antallStr(j.talt_milli)} stk
                  </span>
                </li>
              {/each}
            </ul>
          {/if}
          {#if s.justeringer.length > endringer.length}
            <p class="opacity-70 mb-2">
              {s.justeringer.length - endringer.length} produkt{s.justeringer.length - endringer.length === 1
                ? ""
                : "er"} stemmer med beholdningen.
            </p>
          {/if}
          <p>
            Varelageret står i regnskapet (konto 1460) med {kr(s.bokfort_ore)} kr, og er etter tellingen
            verdt {kr(s.verdi_ore)} kr.
          </p>
          {#if s.endring_ore}
            <p class="mt-1">
              Bilaget fører <span class="font-medium">{kr(Math.abs(s.endring_ore))} kr</span>
              {s.endring_ore > 0 ? "opp" : "ned"} på varelageret, mot beholdningsendring (konto 4390).
            </p>
          {:else}
            <p class="mt-1">Verdien er uendret, så det blir ikke noe bilag.</p>
          {/if}
          <div class="flex gap-2 mt-2">
            <button class="btn btn-sm btn-primary" disabled={bokforer} onclick={bokfor}>
              {s.endring_ore ? "Registrer og bokfør" : "Registrer tellingen"}
            </button>
            <button class="btn btn-sm btn-ghost" onclick={endreTelling}>
              Endre tellingen
            </button>
          </div>
        </div>
      {/if}
    {:else}
      <button
        id="start-telling"
        class="btn btn-sm btn-outline mt-3 self-start"
        onclick={startTelling}
      >Start varetelling</button>

      <h3 class="font-semibold mt-4">Registrer varekjøp eller justering</h3>
      <form class="flex flex-wrap gap-x-3 gap-y-1 items-end text-sm" onsubmit={registrer}>
        <label class="fieldset py-0">
          <span class="fieldset-legend">Produkt</span>
          <select class="select select-sm" value={produkt} onchange={(e) => (ny.produkt = e.currentTarget.value)}>
            {#each lagerforte as p (p.nummer)}
              <option value={p.nummer}>{p.nummer} {p.navn}</option>
            {/each}
          </select>
        </label>
        <label class="fieldset py-0">
          <span class="fieldset-legend">Type</span>
          <select class="select select-sm" bind:value={ny.kind}>
            <option value="kjop">Varekjøp</option>
            <option value="justering">Justering</option>
          </select>
        </label>
        <label class="fieldset py-0">
          <span class="fieldset-legend">Dato</span>
          <input type="date" class="input input-sm" required bind:value={ny.dato} />
        </label>
        <label class="fieldset py-0">
          <span class="fieldset-legend">{ny.kind === "kjop" ? "Antall" : "Antall (+/−)"}</span>
          <input class="input input-sm w-24" inputmode="decimal" required bind:value={ny.antall} />
        </label>
        {#if ny.kind === "kjop"}
          <label class="fieldset py-0">
            <span class="fieldset-legend">Kostpris per stk (kr)</span>
            <input class="input input-sm w-28" inputmode="decimal" required bind:value={ny.kost} />
          </label>
        {/if}
        <label class="fieldset py-0 flex-1 min-w-40">
          <span class="fieldset-legend">{ny.kind === "kjop" ? "Notat (valgfritt)" : "Hvorfor"}</span>
          <input class="input input-sm w-full" required={ny.kind === "justering"} bind:value={ny.note} />
        </label>
        <button class="btn btn-sm btn-primary mb-1" disabled={registrerer || !produkt}>Registrer</button>
      </form>
    {/if}
  {/if}
</Card>
