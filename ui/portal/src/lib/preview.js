// Forhåndsvisning av et dokument (bilag, kvittering) ut fra bytenes EGEN
// signatur (%PDF-, PNG/JPEG/GIF/WebP), aldri typen opplasteren oppga:
// blob-URL-en får typen vi har sjekket, så et dokument kan ikke kjøre
// noe i siden. Alt annet gir { kind: "none" } og lastes ned i stedet.
// Kalleren eier URL-en og må kalle URL.revokeObjectURL når den er ferdig.
export function sniffType(head) {
  const starts = (...b) => b.every((x, i) => head[i] === x);
  if (starts(0x25, 0x50, 0x44, 0x46, 0x2d)) return "application/pdf";
  if (starts(0x89, 0x50, 0x4e, 0x47)) return "image/png";
  if (starts(0xff, 0xd8, 0xff)) return "image/jpeg";
  if (starts(0x47, 0x49, 0x46, 0x38)) return "image/gif";
  if (starts(0x52, 0x49, 0x46, 0x46) && head[8] === 0x57 && head[9] === 0x45) return "image/webp";
  return null;
}

export async function previewOf(blob) {
  const type = sniffType(new Uint8Array(await blob.slice(0, 12).arrayBuffer()));
  if (!type) return { kind: "none" };
  return {
    kind: type === "application/pdf" ? "pdf" : "image",
    url: URL.createObjectURL(new Blob([blob], { type })),
  };
}
