// Markdown in an agent's messages (**fet**, *kursiv*, `kode`, lister,
// overskrifter) -- the models write it whether asked to or not, and shown
// raw it reads as noise: «**Testprosjekt**».
//
// Hand-rolled and small on purpose (the portal is frugality-gated), and
// SAFE by construction: the text is HTML-escaped FIRST, and the only
// markup added afterwards is the fixed set of tags below. No links, no
// images, no raw HTML -- a model's output is untrusted text, and the
// result goes through {@html}.

const ESC = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};
const escape = (s) => s.replace(/[&<>"']/g, (c) => ESC[c]);

function inline(s) {
  // Code is set aside first, so ** inside `code` stays literal while
  // **`code`** is still bold. \u0000 cannot occur in escaped text.
  const code = [];
  const marked = s.replace(
    /`([^`]+)`/g,
    (_, c) => "\u0000" + (code.push(c) - 1) + "\u0000",
  );
  return marked
    .replace(/\*\*(?=\S)(.+?)(?<=\S)\*\*/g, "<strong>$1</strong>")
    .replace(/__(?=\S)(.+?)(?<=\S)__/g, "<strong>$1</strong>")
    .replace(/(^|[^*\w])\*(?=\S)(.+?)(?<=\S)\*(?![*\w])/g, "$1<em>$2</em>")
    .replace(/(^|[^_\w])_(?=\S)(.+?)(?<=\S)_(?![_\w])/g, "$1<em>$2</em>")
    .replace(
      /\u0000(\d+)\u0000/g,
      (_, n) => "<code>" + code[Number(n)] + "</code>",
    );
}

/** Markdown -> safe HTML: escaped text plus a fixed set of tags. */
export function renderMarkdown(text) {
  const lines = escape(String(text ?? "")).split("\n");
  const out = [];
  let list = null; // "ul" | "ol"
  let para = [];
  const flushPara = () => {
    if (para.length) out.push("<p>" + para.map(inline).join("<br>") + "</p>");
    para = [];
  };
  const closeList = () => {
    if (list) out.push("</" + list + ">");
    list = null;
  };
  for (const raw of lines) {
    const line = raw.trimEnd();
    const bullet = line.match(/^\s*[-*•]\s+(.*)$/);
    const number = line.match(/^\s*\d+[.)]\s+(.*)$/);
    const heading = line.match(/^\s*#{1,6}\s+(.*)$/);
    if (bullet || number) {
      flushPara();
      const kind = bullet ? "ul" : "ol";
      if (list !== kind) {
        closeList();
        out.push("<" + kind + ">");
        list = kind;
      }
      out.push("<li>" + inline((bullet || number)[1]) + "</li>");
    } else if (heading) {
      flushPara();
      closeList();
      out.push("<p><strong>" + inline(heading[1]) + "</strong></p>");
    } else if (line.trim() === "") {
      flushPara();
      closeList();
    } else {
      closeList();
      para.push(line);
    }
  }
  flushPara();
  closeList();
  return out.join("");
}
