<script>
  // Searches SSB's occupation catalogue by title or code, so nobody needs
  // to know a seven-digit code. The list is position:fixed for the same
  // reason as in KontoVelger: the card around the form would clip it.
  import { api } from "../../lib/api.js";

  let { companyId, value = $bindable(""), id } = $props();

  let hits = $state([]);
  let open = $state(false);
  let active = $state(-1);
  let inputEl = $state(null);
  let listEl = $state(null);
  let pos = $state({ top: 0, left: 0 });

  $effect(() => {
    const q = value.trim();
    const company = companyId;
    if (!q) {
      hits = [];
      return;
    }
    let current = true;
    const timer = setTimeout(async () => {
      try {
        const answer = await api(
          "/companies/" + company + "/occupations?q=" + encodeURIComponent(q),
        );
        if (current) {
          hits = answer.occupations;
          active = -1;
        }
      } catch {
        if (current) hits = [];
      }
    }, 150);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  });

  const chosen = $derived(hits.find((t) => t.code === value.trim()));
  const visible = $derived(open && hits.length > 0 && !chosen);

  function show() {
    const r = inputEl.getBoundingClientRect();
    // The field sits in the right column: on a phone the list would leave the screen.
    pos = { top: r.bottom + 4, left: Math.max(8, Math.min(r.left, innerWidth - 328)) };
    open = true;
  }

  function choose(t) {
    value = t.code;
    open = false;
  }

  function onKey(e) {
    if (!visible) {
      if ((e.key === "ArrowDown" || e.key === "ArrowUp") && hits.length && !chosen) {
        e.preventDefault();
        show();
      }
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      active = (active + step + hits.length) % hits.length;
      queueMicrotask(() => listEl?.children[active]?.scrollIntoView({ block: "nearest" }));
    } else if (e.key === "Enter" && active >= 0) {
      e.preventDefault();
      choose(hits[active]);
    } else if (e.key === "Escape") {
      e.stopPropagation();
      open = false;
    }
  }
</script>

<svelte:window
  onscrollcapture={(e) => {
    if (!listEl?.contains(e.target)) open = false;
  }}
  onresize={() => (open = false)}
/>

<input
  {id}
  bind:this={inputEl}
  class="input input-sm"
  role="combobox"
  aria-autocomplete="list"
  aria-expanded={visible}
  aria-controls={visible ? id + "-list" : undefined}
  aria-describedby={chosen ? id + "-chosen" : undefined}
  aria-activedescendant={visible && active >= 0 ? id + "-" + active : undefined}
  autocomplete="off"
  placeholder="Søk på tittel eller kode"
  bind:value
  onfocus={show}
  oninput={() => {
    active = -1;
    show();
  }}
  onblur={() => (open = false)}
  onkeydown={onKey}
/>
{#if chosen}
  <span id={id + "-chosen"} class="opacity-70 truncate" title={chosen.name}>{chosen.name}</span>
{/if}
{#if visible}
  <ul
    bind:this={listEl}
    id={id + "-list"}
    role="listbox"
    class="fixed z-50 w-80 max-w-[calc(100vw-16px)] max-h-64 overflow-y-auto p-1 bg-base-100 border border-base-300 rounded-box shadow-lg"
    style="top: {pos.top}px; left: {pos.left}px"
  >
    {#each hits as t, i (t.code)}
      <li
        id={id + "-" + i}
        role="option"
        aria-selected={i === active}
        class="px-2 py-1 rounded cursor-pointer text-sm {i === active
          ? 'bg-base-200'
          : 'hover:bg-base-200'}"
        onmousedown={(e) => {
          e.preventDefault();
          choose(t);
        }}
      >
        <span class="font-mono">{t.code}</span>
        {t.name}
      </li>
    {/each}
  </ul>
{/if}
