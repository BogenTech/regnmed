<script>
  // An address field with suggestions as you type, all from `lookup` (a
  // path taking ?q=): your own earlier addresses first, then the
  // company's customers and suppliers by name (with RESKONTRO_LES), any
  // company in Enhetsregisteret by name, and Kartverket's addresses. Picking a
  // company fills "Name, street, postcode town". The ARIA combobox pattern, not a native
  // <datalist>: a datalist is drawn by the host (detached from the field in
  // some), filters out fuzzy hits, and cannot be themed.
  //
  // Keyboard: ↓/↑ move through the suggestions, Enter picks, Escape closes.
  // Free text is always allowed — a suggestion is a convenience.
  import { api } from "../lib/api.js";

  let { id, value = $bindable(""), lookup, placeholder = "", invalid = false } = $props();

  let options = $state([]);
  let open = $state(false);
  let active = $state(-1);
  let timer;
  let input = $state(null);
  const listId = id + "-forslag";

  function update() {
    clearTimeout(timer);
    active = -1;
    const t = value.trim();
    timer = setTimeout(() => {
      api(lookup + "?q=" + encodeURIComponent(t))
        .then((r) => {
          // A late answer for text the user has already changed is dropped.
          if (value.trim() !== t) return;
          const seen = new Set();
          options = r.addresses.filter((o) => text(o) !== value && !seen.has(text(o)) && seen.add(text(o)));
          // An answer that lands after the user tabbed away must not
          // reopen the list over the next field.
          open = options.length > 0 && document.activeElement === input;
        })
        .catch(() => {});
    }, t ? 250 : 0);
  }

  const text = (o) => (o.name ? o.name + ", " + o.address : o.address);

  function pick(option) {
    value = text(option);
    open = false;
    active = -1;
  }

  function keydown(event) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      if (!options.length) return;
      event.preventDefault();
      open = true;
      const step = event.key === "ArrowDown" ? 1 : -1;
      active = (active + step + options.length) % options.length;
    } else if (event.key === "Enter" && open && active >= 0) {
      event.preventDefault(); // pick, don't submit the form
      pick(options[active]);
    } else if (event.key === "Escape" && open) {
      event.preventDefault();
      open = false;
    }
  }
</script>

<div class="relative">
  <input
    bind:this={input}
    {id}
    class="input w-full"
    role="combobox"
    aria-autocomplete="list"
    aria-expanded={open}
    aria-controls={listId}
    aria-activedescendant={open && active >= 0 ? listId + "-" + active : undefined}
    aria-invalid={invalid}
    autocomplete="off"
    {placeholder}
    bind:value
    oninput={update}
    onkeydown={keydown}
    onfocus={() => !value && update()}
    onblur={() => (clearTimeout(timer), (open = false))}
  />
  {#if open}
    <ul
      id={listId}
      role="listbox"
      aria-label="Adresseforslag"
      class="absolute z-20 mt-1 w-full max-h-64 overflow-y-auto rounded-box border border-base-300 bg-base-100 shadow-lg p-1"
    >
      {#each options as option, i (text(option))}
        <li
          id={listId + "-" + i}
          role="option"
          aria-selected={i === active}
          class="px-3 py-2 rounded-field cursor-pointer text-sm {i === active ? 'bg-primary text-primary-content' : 'hover:bg-base-200'}"
          onmousedown={(e) => (e.preventDefault(), pick(option))}
        >
          {#if option.name}
            <span class="font-medium">{option.name}</span>
            <span class="block text-xs {i === active ? '' : 'opacity-70'}">{option.address}</span>
          {:else}
            {option.address}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>
