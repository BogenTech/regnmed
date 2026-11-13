<script>
  // The driving route of a trip on a map, in a dialog. Leaflet is loaded
  // only when the dialog opens, and the tiles are Kartverket's open
  // topographic map (CSP: img-src cache.kartverket.no, nothing else).
  // `load` returns the route from our API: { line, from, to, km }.
  //
  // Keyboard: the button opens, Escape or «Lukk» closes (native <dialog>),
  // and Leaflet's own map takes arrow keys to pan and +/- to zoom once
  // focused.
  let { load, label = "Vis kart" } = $props();

  let dialog = $state(null);
  let holder = $state(null);
  let error = $state(null);
  let km = $state(null);
  let map = null;

  async function open() {
    error = null;
    dialog.showModal();
    try {
      const [route, leaflet] = await Promise.all([load(), import("leaflet"), import("leaflet/dist/leaflet.css")]);
      const L = leaflet.default;
      km = route.km;
      map?.remove();
      map = L.map(holder, { keyboard: true });
      L.tileLayer("https://cache.kartverket.no/v1/wmts/1.0.0/topo/default/webmercator/{z}/{y}/{x}.png", {
        maxZoom: 18,
        attribution: "© Kartverket",
      }).addTo(map);
      const line = L.polyline(route.line, { weight: 5 }).addTo(map);
      L.circleMarker(route.from, { radius: 7, fillOpacity: 1 }).bindTooltip("Fra").addTo(map);
      L.circleMarker(route.to, { radius: 7, fillOpacity: 1 }).bindTooltip("Til").addTo(map);
      map.fitBounds(line.getBounds(), { padding: [24, 24] });
    } catch (e) {
      error = e.message;
    }
  }
</script>

<button type="button" class="btn btn-sm" onclick={open}>{label}</button>

<dialog bind:this={dialog} class="modal" aria-labelledby="rutekart-tittel" onclose={() => (map?.remove(), (map = null))}>
  <div class="modal-box max-w-3xl w-full grid gap-3">
    <h2 id="rutekart-tittel" class="font-bold text-lg">Kjøreruten{km ? " — " + km + " km én vei" : ""}</h2>
    {#if error}
      <div class="alert alert-error" role="alert"><span>{error}</span></div>
    {/if}
    <div bind:this={holder} class="h-[60vh] w-full rounded-box" aria-label="Kart over kjøreruten"></div>
    <div class="modal-action mt-0">
      <!-- Buttons, not <form method="dialog">: the map sits inside the trip form. -->
      <button type="button" class="btn" onclick={() => dialog.close()}>Lukk</button>
    </div>
  </div>
  <button type="button" class="modal-backdrop" tabindex="-1" aria-label="Lukk kartet" onclick={() => dialog.close()}></button>
</dialog>
