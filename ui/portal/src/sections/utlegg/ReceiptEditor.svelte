<script>
  // Rotate and crop a receipt photo before it becomes evidence. Plain
  // canvas, no library. Crop by dragging on the picture, or with the four
  // edge sliders (the keyboard way). The result is a JPEG, scaled down to
  // MAX px on the long edge: a receipt needs to be legible, not 12 MP.
  let { url, onsave, oncancel } = $props();

  const MAX = 2400;
  let img = $state(null);
  let rotation = $state(0); // 0, 90, 180, 270
  // Edges in percent of the ROTATED picture.
  let crop = $state({ left: 0, top: 0, right: 0, bottom: 0 });
  let canvas = $state(null);
  let busy = $state(false);
  let first = $state(null);

  // Keyboard users land in the editor, and Escape leaves it.
  $effect(() => first?.focus());

  $effect(() => {
    const i = new Image();
    i.onload = () => (img = i);
    i.src = url;
  });

  let rotated = $derived(
    img && rotation % 180 ? { w: img.naturalHeight, h: img.naturalWidth } : { w: img?.naturalWidth, h: img?.naturalHeight },
  );

  // Draws the whole rotated picture into `target`, scaled by `scale`.
  function drawRotated(target, scale) {
    const ctx = target.getContext("2d");
    ctx.save();
    ctx.translate(target.width / 2, target.height / 2);
    ctx.rotate((rotation * Math.PI) / 180);
    ctx.drawImage(img, (-img.naturalWidth * scale) / 2, (-img.naturalHeight * scale) / 2, img.naturalWidth * scale, img.naturalHeight * scale);
    ctx.restore();
  }

  $effect(() => {
    if (!img || !canvas) return;
    const scale = Math.min(1, 900 / Math.max(rotated.w, rotated.h));
    canvas.width = Math.round(rotated.w * scale);
    canvas.height = Math.round(rotated.h * scale);
    drawRotated(canvas, scale);
    // Dim what will be cut away.
    const ctx = canvas.getContext("2d");
    const { left, top, right, bottom } = crop;
    const W = canvas.width, H = canvas.height;
    const x0 = (left / 100) * W, y0 = (top / 100) * H, x1 = W - (right / 100) * W, y1 = H - (bottom / 100) * H;
    ctx.fillStyle = "rgba(0,0,0,0.55)";
    ctx.fillRect(0, 0, W, y0);
    ctx.fillRect(0, y1, W, H - y1);
    ctx.fillRect(0, y0, x0, y1 - y0);
    ctx.fillRect(x1, y0, W - x1, y1 - y0);
    ctx.strokeStyle = "#fff";
    ctx.lineWidth = 2;
    ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
  });

  function rotate(by) {
    rotation = (rotation + by + 360) % 360;
    crop = { left: 0, top: 0, right: 0, bottom: 0 };
  }

  // Drag draws a new crop rectangle.
  let start = null;
  const pct = (event) => {
    const r = canvas.getBoundingClientRect();
    return {
      x: Math.min(100, Math.max(0, ((event.clientX - r.left) / r.width) * 100)),
      y: Math.min(100, Math.max(0, ((event.clientY - r.top) / r.height) * 100)),
    };
  };
  function down(event) {
    start = pct(event);
    canvas.setPointerCapture(event.pointerId);
  }
  function move(event) {
    if (!start) return;
    const p = pct(event);
    if (Math.abs(p.x - start.x) < 2 || Math.abs(p.y - start.y) < 2) return;
    crop = {
      left: Math.min(start.x, p.x),
      top: Math.min(start.y, p.y),
      right: 100 - Math.max(start.x, p.x),
      bottom: 100 - Math.max(start.y, p.y),
    };
  }
  const up = () => (start = null);

  // An edge never passes the opposite one: at least 10 % stays.
  const limit = (edge) => 90 - crop[{ left: "right", right: "left", top: "bottom", bottom: "top" }[edge]];

  async function save() {
    busy = true;
    const full = document.createElement("canvas");
    full.width = rotated.w;
    full.height = rotated.h;
    drawRotated(full, 1);
    const sx = (crop.left / 100) * rotated.w, sy = (crop.top / 100) * rotated.h;
    const sw = rotated.w - sx - (crop.right / 100) * rotated.w, sh = rotated.h - sy - (crop.bottom / 100) * rotated.h;
    const scale = Math.min(1, MAX / Math.max(sw, sh));
    const out = document.createElement("canvas");
    out.width = Math.round(sw * scale);
    out.height = Math.round(sh * scale);
    out.getContext("2d").drawImage(full, sx, sy, sw, sh, 0, 0, out.width, out.height);
    out.toBlob(async (blob) => {
      try {
        await onsave(blob);
      } finally {
        busy = false;
      }
    }, "image/jpeg", 0.9);
  }
</script>

<div class="grid gap-3" role="group" aria-label="Roter og beskjær kvitteringen" onkeydown={(e) => e.key === "Escape" && oncancel()}>
  {#if !img}
    <div class="flex justify-center p-12"><span class="loading loading-spinner"></span></div>
  {:else}
    <canvas
      bind:this={canvas}
      class="w-full max-h-[60vh] object-contain touch-none cursor-crosshair rounded"
      aria-label="Kvitteringen. Dra over bildet for å beskjære, eller bruk kantene under."
      role="img"
      onpointerdown={down}
      onpointermove={move}
      onpointerup={up}
      onpointercancel={up}
    ></canvas>
    <div class="flex flex-wrap gap-2">
      <button bind:this={first} type="button" class="btn btn-sm" onclick={() => rotate(-90)}>↺ Roter mot venstre</button>
      <button type="button" class="btn btn-sm" onclick={() => rotate(90)}>↻ Roter mot høyre</button>
      <button type="button" class="btn btn-sm btn-ghost" onclick={() => (crop = { left: 0, top: 0, right: 0, bottom: 0 })}>Hele bildet</button>
    </div>
    <details>
      <summary class="cursor-pointer text-sm">Beskjær med tastaturet</summary>
      <div class="grid gap-2 sm:grid-cols-2 mt-2">
        {#each [["left", "Venstre kant"], ["right", "Høyre kant"], ["top", "Øvre kant"], ["bottom", "Nedre kant"]] as [edge, label] (edge)}
          <label class="text-sm grid gap-1">
            {label}: {Math.round(crop[edge])} %
            <input type="range" class="range range-xs" min="0" max={limit(edge)} bind:value={crop[edge]} />
          </label>
        {/each}
      </div>
    </details>
    <div class="flex flex-wrap gap-2">
      <button type="button" class="btn btn-primary" disabled={busy} onclick={save}>
        {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
        Bruk dette bildet
      </button>
      <button type="button" class="btn btn-ghost" onclick={oncancel}>Avbryt</button>
    </div>
  {/if}
</div>
