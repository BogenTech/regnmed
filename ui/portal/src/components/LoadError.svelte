<script>
  // A fetch that failed says so where the content would have been, with
  // a way to try again — never a spinner that spins forever.
  let { error, retry } = $props();
</script>

<div role="alert" class="alert alert-error alert-soft text-sm">
  <span>{error}</span>
  {#if retry}<button class="btn btn-sm" onclick={retry}>Prøv igjen</button>{/if}
</div>
