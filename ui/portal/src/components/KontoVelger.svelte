<script>
  // Account picker: the user should never need to know a number by
  // heart. Suggestions come from the company's own accounts first, then
  // the standard catalog (marked "standard" — picking one of those means
  // the caller adds it to the company before posting). Free typing is
  // still allowed, so a custom account number can be entered directly.
  //
  // The list is position:fixed and measured from the input: the form
  // lives inside a card whose body is overflow-x-auto (wide tables
  // scroll there), and any overflow container would clip an absolutely
  // positioned dropdown. Fixed positioning escapes it; the list closes
  // on scroll/resize so it cannot drift away from its input.
  let { kontoer = [], standard = [], value = $bindable(""), cls = "input input-sm w-24" } = $props();

  let open = $state(false);
  let inputEl = $state(null);
  let pos = $state({ top: 0, left: 0 });

  function vis() {
    if (!inputEl) return;
    const r = inputEl.getBoundingClientRect();
    pos = { top: r.bottom + 4, left: r.left };
    open = true;
  }

  let forslag = $derived.by(() => {
    const q = value.trim().toLowerCase();
    if (!q) return [];
    const egneNr = new Set(kontoer.map((k) => k.number));
    const treff = (nr, navn) => nr.startsWith(q) || navn.toLowerCase().includes(q);
    const egne = kontoer
      .filter((k) => k.active && treff(k.number, k.name))
      .map((k) => ({ number: k.number, name: k.name, standard: false }));
    const std = standard
      .filter((s) => !egneNr.has(s.number) && treff(s.number, s.name))
      .map((s) => ({ ...s, standard: true }));
    return [...egne, ...std].slice(0, 12);
  });

  function velg(f) {
    value = f.number;
    open = false;
  }
</script>

<svelte:window onscrollcapture={() => (open = false)} onresize={() => (open = false)} />

<input
  bind:this={inputEl}
  class={cls}
  placeholder="Konto"
  bind:value
  onfocus={vis}
  oninput={vis}
  onblur={() => setTimeout(() => (open = false), 150)}
/>
{#if open && forslag.length && !(forslag.length === 1 && forslag[0].number === value.trim())}
  <ul
    class="fixed z-50 w-72 max-h-64 overflow-y-auto menu menu-sm bg-base-100 border border-base-300 rounded-box shadow-lg flex-nowrap"
    style="top: {pos.top}px; left: {pos.left}px"
  >
    {#each forslag as f (f.number)}
      <li>
        <button type="button" class="justify-between gap-2" onmousedown={() => velg(f)}>
          <span><span class="font-mono">{f.number}</span> {f.name}</span>
          {#if f.standard}<span class="badge badge-ghost badge-xs">standard</span>{/if}
        </button>
      </li>
    {/each}
  </ul>
{/if}
