<script>
  // Repeterende fakturaer (#30): genereres av den nattlige kjøringen;
  // «Generer nå» tar forfalte perioder med det samme.
  import { post, send } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, templates, onDone } = $props();

  const INTERVALL_NAVN = { manedlig: "månedlig", kvartalsvis: "kvartalsvis", arlig: "årlig" };

  async function generer(t) {
    try {
      const result = await post(
        "/companies/" + companyId + "/invoice-templates/" + t.template_id + "/generate",
        {},
      );
      const count = result.generated.length;
      toast(count ? count + " faktura(er) generert" : "Ingen forfalte perioder", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function toggle(t) {
    try {
      await send("PUT", "/companies/" + companyId + "/invoice-templates/" + t.template_id, {
        active: !t.active,
      });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Repeterende fakturaer">
  <p class="text-sm opacity-70 mb-2">
    Genereres automatisk hver natt når neste dato passeres — som ordinære fakturaer med
    løpenummer, KID og PDF. Bruk {"{måned}"} og {"{år}"} i linjeteksten for periodetekst.
  </p>
  <table class="table table-sm">
    <thead>
      <tr>
        <th>Kunde</th><th>Intervall</th><th>Neste</th>
        <th class="text-right">Netto</th><th>Kjøringer</th><th></th>
      </tr>
    </thead>
    <tbody>
      {#each templates as t (t.template_id)}
        <tr class={t.active ? "" : "opacity-50"}>
          <td>{t.party_name}</td>
          <td>{INTERVALL_NAVN[t.intervall] || t.intervall}</td>
          <td>
            {t.neste_dato}
            {#if t.slutt_dato}<span class="opacity-60">(til {t.slutt_dato})</span>{/if}
          </td>
          <td class="text-right">{kr(t.sum_netto_ore)}</td>
          <td>
            {t.runs}
            {#if t.merk_utsendelse}<span class="badge badge-ghost badge-xs">utsendelse</span>{/if}
          </td>
          <td>
            <button class="btn btn-xs btn-outline" onclick={() => generer(t)}>Generer nå</button>
            <button class="btn btn-xs btn-ghost" onclick={() => toggle(t)}>
              {t.active ? "Stopp" : "Start"}
            </button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</Card>
