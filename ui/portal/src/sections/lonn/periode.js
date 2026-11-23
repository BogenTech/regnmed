// Which month the payroll form offers, and how a payslip file is named.

// The month after the latest run — payroll is run month by month, so that
// is the next one due. With no run yet, the current month. A form that
// opened on «januar» in November was one click from booking the wrong
// month.
export function nextPeriod(runs, todayIso) {
  const latest = latestRun(runs);
  if (!latest) return { year: Number(todayIso.slice(0, 4)), month: Number(todayIso.slice(5, 7)) };
  return latest.maned === 12
    ? { year: latest.ar + 1, month: 1 }
    : { year: latest.ar, month: latest.maned + 1 };
}

function latestRun(runs) {
  return runs.reduce(
    (best, k) => (!best || k.ar * 12 + k.maned > best.ar * 12 + best.maned ? k : best),
    null,
  );
}

// The latest run's zone, or none: a guessed zone on a first run would post
// the wrong employer contribution to an append-only ledger.
export function startingZone(runs) {
  return latestRun(runs)?.sone ?? "";
}

// «lonnsslipp-2026-06-kari-utvikler.pdf»: one file per person and month,
// so a folder of them can be told apart.
export function slipFilename(year, month, name) {
  const slug = name
    .toLowerCase()
    .replace(/æ/g, "ae")
    .replace(/ø/g, "o")
    .replace(/å/g, "a")
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
  return `lonnsslipp-${year}-${String(month).padStart(2, "0")}-${slug}.pdf`;
}
