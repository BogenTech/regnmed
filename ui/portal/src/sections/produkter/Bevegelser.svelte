<script>
  // Bevegelseslisten for ett produkt — salgslinjene bærer fakturaen de
  // kom fra, så beholdningen alltid kan spores tilbake til et bilag.
  import { kr, antallStr } from "../../lib/format.js";

  let { nummer, movements } = $props();

  const KIND_NAVN = { kjop: "varekjøp", salg: "salg", justering: "justering" };
</script>

<h3 class="font-semibold mb-1">Bevegelser — {nummer}</h3>
<table class="table table-xs">
  <thead>
    <tr>
      <th>Dato</th><th>Type</th><th class="text-right">Antall</th>
      <th class="text-right">Kost/stk</th><th>Notat</th>
    </tr>
  </thead>
  <tbody>
    {#each movements as m}
      <tr>
        <td>{m.dato}</td>
        <td>
          {KIND_NAVN[m.kind] || m.kind}
          {#if m.invoice_no}
            <span class="opacity-60">(faktura {m.invoice_no})</span>
          {/if}
        </td>
        <td class="text-right">{antallStr(m.antall_milli)}</td>
        <td class="text-right">{m.kostpris_ore == null ? "–" : kr(m.kostpris_ore)}</td>
        <td>{m.note || ""}</td>
      </tr>
    {/each}
  </tbody>
</table>
