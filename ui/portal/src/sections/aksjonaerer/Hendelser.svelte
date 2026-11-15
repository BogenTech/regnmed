<script>
  // Hendelser er innsettings-bare, som bilag: en feilføring rettes med en
  // motsatt hendelse, aldri ved å endre historien. So the form says in one
  // sentence what will be written and asks before writing it. A transfer
  // (kjøp, salg, overføring, ...) always names the other side, which the
  // server writes in the same transaction — without it the company's
  // share total would move.
  import { post } from "../../lib/api.js";
  import { formatDate, kr, oreOrNull, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import { grupperTyper, hendelseTekst, lesAksjer } from "./aksjonaer.js";

  let { companyId, hendelser, aksjonarer, typer, kanSkrive, onChanged } = $props();

  let grupper = $derived(grupperTyper(typer));
  const tomt = () => ({ holder: "", type: "", dato: today(), antall: "", belop: "", motpart: "" });
  let felt = $state(tomt());
  let lagrer = $state(false);

  // The pickers always point at something that exists.
  let holder = $derived(
    aksjonarer.some((a) => a.id === felt.holder) ? felt.holder : (aksjonarer[0]?.id ?? ""),
  );
  let type = $derived(typer.find((t) => t.slug === felt.type) ?? typer[0]);
  let medMotpart = $derived(!!type?.motpart_type);
  let motparter = $derived(aksjonarer.filter((a) => a.id !== holder));
  // A choice that is no longer offered (the shareholder changed) is no choice.
  let motpart = $derived(motparter.some((a) => a.id === felt.motpart) ? felt.motpart : "");
  const navn = (id) => aksjonarer.find((a) => a.id === id)?.navn ?? "";

  async function registrer(event) {
    event.preventDefault();
    if (lagrer || !type) return;
    const aksjer = lesAksjer(felt.antall);
    if (aksjer.feil) return toast(aksjer.feil, false);
    let belop = null;
    if (felt.belop.trim()) {
      belop = oreOrNull(felt.belop);
      if (belop == null || belop < 0)
        return toast("Beløpet må være et beløp i kroner, for eksempel 60 000", false);
    }
    if (medMotpart && !motpart)
      return toast("Velg aksjonæren aksjene " + (type.tilgang ? "kommer fra" : "går til"), false);
    const tekst = hendelseTekst({
      type,
      antall: aksjer.antall,
      holder: navn(holder),
      motpart: medMotpart ? navn(motpart) : null,
      dato: felt.dato,
    });
    const ok = await bekreft(
      tekst +
        (belop != null ? " Beløp " + kr(belop) + " kr." : "") +
        " En hendelse kan ikke endres etterpå — en feil rettes med en motsatt hendelse.",
      { tittel: "Registrer hendelsen?", ok: "Registrer" },
    );
    if (!ok) return;
    lagrer = true;
    try {
      await post("/companies/" + companyId + "/share-events", {
        shareholder_id: holder,
        type: type.slug,
        dato: felt.dato,
        antall: aksjer.antall,
        belop_ore: belop,
        motpart_id: medMotpart ? motpart : null,
      });
      toast("Hendelsen er registrert", true);
      felt = { ...tomt(), holder: felt.holder, type: felt.type };
      await onChanged();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }
</script>

<Card title="Hendelser">
  <p class="text-sm opacity-70 mb-2">
    Alt som endrer eierskapet: stiftelse, emisjoner, kjøp og salg. En overdragelse registreres én
    gang, og begge sider skrives samtidig. ⚠ merker typer som ennå ikke kan leveres til
    Skatteetaten.
  </p>
  {#if hendelser.length}
    <ul class="divide-y divide-base-200 text-sm">
      {#each hendelser as h, i (i)}
        <li class="py-2 flex gap-3">
          <span class="whitespace-nowrap w-20 shrink-0">{formatDate(h.dato)}</span>
          <div class="min-w-0 flex-1">
            <div>{h.aksjonar}: {h.type_navn}</div>
            {#if h.motpart || h.belop_ore != null}
              <div class="text-xs opacity-60">
                {[h.motpart && "motpart " + h.motpart, h.belop_ore != null && kr(h.belop_ore) + " kr"]
                  .filter(Boolean)
                  .join(", ")}
              </div>
            {/if}
          </div>
          <span class="whitespace-nowrap text-right">{h.antall.toLocaleString("nb-NO")}</span>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-sm opacity-70">Ingen hendelser ennå. Start med stiftelsen.</p>
  {/if}

  {#if kanSkrive && aksjonarer.length}
    <h3 class="font-semibold mt-4">Ny hendelse</h3>
    <form class="flex flex-wrap gap-x-3 gap-y-1 items-end text-sm" onsubmit={registrer}>
      <label class="fieldset py-0 min-w-48">
        <span class="fieldset-legend">Aksjonær</span>
        <select
          class="select select-sm"
          value={holder}
          onchange={(e) => (felt.holder = e.currentTarget.value)}
        >
          {#each aksjonarer as a (a.id)}
            <option value={a.id}>{a.navn} ({a.antall_aksjer.toLocaleString("nb-NO")})</option>
          {/each}
        </select>
      </label>
      <label class="fieldset py-0 min-w-48">
        <span class="fieldset-legend">Hendelse</span>
        <select
          class="select select-sm"
          value={type?.slug}
          onchange={(e) => (felt.type = e.currentTarget.value)}
        >
          <optgroup label="Aksjer inn">
            {#each grupper.tilganger as t (t.slug)}
              <option value={t.slug}>{t.visning}</option>
            {/each}
          </optgroup>
          <optgroup label="Aksjer ut">
            {#each grupper.avganger as t (t.slug)}
              <option value={t.slug}>{t.visning}</option>
            {/each}
          </optgroup>
        </select>
      </label>
      <label class="fieldset py-0">
        <span class="fieldset-legend">Dato</span>
        <input type="date" class="input input-sm" required bind:value={felt.dato} />
      </label>
      <label class="fieldset py-0">
        <span class="fieldset-legend">Antall aksjer</span>
        <input class="input input-sm w-28" inputmode="numeric" required bind:value={felt.antall} />
      </label>
      <label class="fieldset py-0">
        <span class="fieldset-legend">Beløp (kr, valgfritt)</span>
        <input class="input input-sm w-32" inputmode="decimal" bind:value={felt.belop} />
      </label>
      {#if medMotpart}
        <label class="fieldset py-0 min-w-48">
          <span class="fieldset-legend">{type.tilgang ? "Fra" : "Til"}</span>
          <select
            class="select select-sm"
            required
            value={motpart}
            onchange={(e) => (felt.motpart = e.currentTarget.value)}
          >
            <option value="" disabled>Velg aksjonær</option>
            {#each motparter as a (a.id)}
              <option value={a.id}>{a.navn}</option>
            {/each}
          </select>
        </label>
      {/if}
      <button class="btn btn-sm btn-primary mb-1" disabled={lagrer}>Registrer hendelse</button>
    </form>
    {#if medMotpart && !motparter.length}
      <p class="text-xs opacity-70 mt-1">
        Registrer først aksjonæren på den andre siden — begge sider må stå i boken.
      </p>
    {/if}
  {/if}
</Card>
