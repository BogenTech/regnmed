<script>
  // Repeterende fakturaer (#30): genereres av den nattlige kjøringen;
  // «Lag forfalte nå» tar forfalte perioder med det samme. Generating
  // issues real invoices, which cannot be undone, so it asks first.
  import { post, send } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, templates, onDone } = $props();

  const INTERVALS = [
    ["manedlig", "Hver måned"],
    ["kvartalsvis", "Hvert kvartal"],
    ["arlig", "Hvert år"],
  ];
  const intervalName = (key) => INTERVALS.find(([k]) => k === key)?.[1] || key;
  const base = (t) => "/companies/" + companyId + "/invoice-templates/" + t.template_id;

  async function generer(t) {
    const ok = await bekreft(
      "Fakturaer til " + t.party_name + " lages for hver periode som er forfalt (neste er " +
        formatDate(t.neste_dato) + "), " + kr(t.sum_netto_ore) + " kr pluss mva hver. " +
        "Utstedte fakturaer kan ikke endres.",
      { tittel: "Lage forfalte fakturaer nå?", ok: "Lag fakturaene" },
    );
    if (!ok) return;
    try {
      const result = await post(base(t) + "/generate", {});
      const count = result.generated.length;
      toast(count ? count + (count === 1 ? " faktura er laget" : " fakturaer er laget") : "Ingen perioder er forfalt ennå", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function endre(t) {
    const answer = await skjema(
      "Endre repeterende faktura til " + t.party_name,
      [
        { navn: "intervall", etikett: "Hvor ofte", type: "select", standard: t.intervall, valg: INTERVALS },
        { navn: "neste", etikett: "Neste faktura lages", type: "date", standard: t.neste_dato },
        {
          navn: "slutt",
          etikett: "Siste periode (valgfritt)",
          type: "date",
          standard: t.slutt_dato || "",
          valgfri: true,
          hjelp: "Tomt betyr at den fortsetter til du stopper den.",
        },
        { navn: "forfall", etikett: "Betalingsfrist (dager)", standard: String(t.forfall_dager ?? 14) },
      ],
      { ok: "Lagre" },
    );
    if (!answer) return;
    const days = Number(answer.forfall);
    if (!Number.isInteger(days) || days < 0) return toast("Betalingsfristen må være et helt antall dager", false);
    try {
      await send("PUT", base(t), {
        intervall: answer.intervall,
        neste_dato: answer.neste,
        slutt_dato: answer.slutt || null,
        forfall_dager: days,
      });
      toast("Lagret", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function toggle(t) {
    try {
      await send("PUT", base(t), { active: !t.active });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<p class="text-sm opacity-70 mb-4">
  Lages automatisk hver natt når neste dato er passert, som vanlige fakturaer med løpenummer, KID og
  PDF. Skriv {"{måned}"} eller {"{år}"} i linjeteksten for å få perioden med på fakturaen.
</p>

<Card title="Maler">
  <div class="overflow-x-auto">
    <table class="table table-sm">
      <caption class="sr-only">Repeterende fakturaer per kunde.</caption>
      <thead>
        <tr>
          <th scope="col">Kunde</th><th scope="col">Hvor ofte</th><th scope="col">Neste</th>
          <th scope="col" class="text-right">Netto</th><th scope="col" class="hidden sm:table-cell">Laget</th>
          <th scope="col"><span class="sr-only">Handlinger</span></th>
        </tr>
      </thead>
      <tbody>
        {#each templates as t (t.template_id)}
          <tr class={t.active ? "" : "opacity-60"}>
            <td>
              {t.party_name}
              {#if !t.active}<span class="badge badge-ghost badge-sm">Stoppet</span>{/if}
            </td>
            <td>{intervalName(t.intervall)}</td>
            <td class="whitespace-nowrap">
              {formatDate(t.neste_dato)}
              {#if t.slutt_dato}<span class="block text-xs opacity-70">til {formatDate(t.slutt_dato)}</span>{/if}
            </td>
            <td class="text-right tabular-nums whitespace-nowrap">{kr(t.sum_netto_ore)}</td>
            <td class="hidden sm:table-cell">
              {t.runs} {t.runs === 1 ? "faktura" : "fakturaer"}
              {#if t.merk_utsendelse}<span class="block text-xs opacity-70">merkes for utsending</span>{/if}
            </td>
            <td>
              <div class="flex flex-wrap gap-1 justify-end">
                {#if t.active}
                  <button class="btn btn-xs btn-outline" onclick={() => generer(t)}>Lag forfalte nå</button>
                {/if}
                <button class="btn btn-xs btn-ghost" aria-label={"Endre repeterende faktura til " + t.party_name} onclick={() => endre(t)}>Endre</button>
                <button class="btn btn-xs btn-ghost" onclick={() => toggle(t)}>{t.active ? "Stopp" : "Start igjen"}</button>
              </div>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</Card>
