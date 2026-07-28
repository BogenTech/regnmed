<script>
  // Midlertidig: seksjoner som ennå ikke er portert til Svelte lenkes
  // til dagens portal på samme adresse. Slettes seksjon for seksjon
  // etter hvert som migreringen (#76) skrider fram.
  let { companyId, section, label } = $props();
</script>

<div class="card bg-base-100 shadow-sm mb-6">
  <div class="card-body">
    <h2 class="card-title">{label}</h2>
    <p class="opacity-70 text-sm">
      Denne seksjonen er ikke flyttet til den nye portalen ennå.
    </p>
    <a class="btn btn-primary btn-sm w-fit" href={"/#/c/" + companyId + "/" + section}>
      Åpne {label} i dagens portal
    </a>
  </div>
</div>
