// Preview of a document (voucher, receipt) from the bytes' OWN signature
// (%PDF-, PNG/JPEG/GIF/WebP), never the type the uploader declared: the
// blob URL gets the type we checked, so a document cannot run anything
// in the page. Anything else is { kind: "none" } and is downloaded
// instead. The caller owns the URL and must revoke it when done.
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
