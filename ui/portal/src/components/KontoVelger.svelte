<script>
  // Account picker: the user should never need to know a number by
  // heart. Suggestions come from the company's own accounts first, then
  // the standard catalog (marked "standard" — picking one of those means
  // the caller adds it to the company before posting). Free typing is
  // still allowed, so a custom account number can be entered directly.
  let { kontoer = [], standard = [], value = $bindable(""), cls = "input input-sm w-24" } = $props();

  let open = $state(false);

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

<div class="relative">
  <input
    class={cls}
    placeholder="Konto"
    bind:value
    onfocus={() => (open = true)}
    oninput={() => (open = true)}
    onblur={() => setTimeout(() => (open = false), 150)}
  />
  {#if open && forslag.length && !(forslag.length === 1 && forslag[0].number === value.trim())}
    <ul
      class="absolute z-30 mt-1 w-72 max-h-64 overflow-y-auto menu menu-sm bg-base-100 border border-base-300 rounded-box shadow-lg flex-nowrap"
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
</div>
