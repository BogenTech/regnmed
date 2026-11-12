// regnmed-core::kid::is_valid, mirrored so a form can mark a mistyped or
// misread KID before the server refuses the whole booking over it. The
// server stays authoritative.

function checkDigit(base, weight, finish) {
  let sum = 0;
  [...base].reverse().forEach((ch, i) => (sum += weight(Number(ch), i)));
  return finish(sum);
}

const mod10 = (base) =>
  checkDigit(
    base,
    (d, i) => {
      const w = i % 2 === 0 ? d * 2 : d;
      return Math.floor(w / 10) + (w % 10);
    },
    (sum) => (10 - (sum % 10)) % 10,
  );

const mod11 = (base) =>
  checkDigit(
    base,
    (d, i) => d * (2 + (i % 6)),
    (sum) => (sum % 11 === 0 ? 0 : sum % 11 === 1 ? null : 11 - (sum % 11)),
  );

export function isValidKid(kid) {
  if (kid.length < 2 || kid.length > 25 || !/^\d+$/.test(kid)) return false;
  const base = kid.slice(0, -1);
  const last = Number(kid.at(-1));
  return mod10(base) === last || mod11(base) === last;
}
