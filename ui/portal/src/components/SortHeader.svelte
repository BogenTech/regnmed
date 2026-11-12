<script>
  // A sortable column header: a real button (Tab + Enter/Space), and
  // aria-sort on the header cell so a screen reader hears the order.
  let { label, key, sort, desc, onsort, class: cls = "" } = $props();
  let active = $derived(sort === key);
</script>

<th scope="col" class={cls} aria-sort={active ? (desc ? "descending" : "ascending") : "none"}>
  <button type="button" class="inline-flex items-center gap-1 font-semibold hover:underline" onclick={() => onsort(key)}>
    {label}
    <span aria-hidden="true" class={active ? "" : "opacity-0"}>{active && desc ? "↓" : "↑"}</span>
  </button>
</th>
