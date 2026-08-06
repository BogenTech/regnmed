<script>
  let { title, children } = $props();
</script>

<!-- overflow-x-auto på kortkroppen: brede tabeller ruller i sitt eget
     felt i stedet for å dytte hele siden sidelengs på en telefon. -->
<div class="card card-border bg-base-100 mb-6">
  <div class="card-body overflow-x-auto">
    <h2 class="card-title">{title}</h2>
    {@render children?.()}
  </div>
</div>
