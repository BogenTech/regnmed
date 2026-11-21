// Background jobs (docs/jobs.md): start one, then wait for its result
// without holding a request open. The server answers 202 at once; the work
// runs in `regnmed work`, and this polls the job until it is finished --
// quickly at first, then less often, so a long import costs few requests.

import { api } from "./api.js";

export async function runJob(companyId, kind, options = {}, onStatus = () => {}) {
  const started = await api("/companies/" + companyId + "/jobs/" + kind, {
    method: "POST",
    ...options,
  });
  onStatus(started.status);
  return waitForJob(companyId, started.job_id, onStatus);
}

// For a job someone is actively waiting on, which usually takes a second or two.
export const FAST_POLL = { first: 75, growth: 1.25, max: 400 };

export async function waitForJob(
  companyId,
  jobId,
  onStatus = () => {},
  { first = 300, growth = 1.5, max = 3000, timeoutMs = null, timeoutMessage } = {},
) {
  const started = Date.now();
  let wait = first;
  for (;;) {
    if (timeoutMs !== null && Date.now() - started >= timeoutMs) {
      throw new Error(timeoutMessage || "Jobben tar lengre tid enn ventet.");
    }
    await new Promise((r) => setTimeout(r, wait));
    wait = Math.min(wait * growth, max);
    const job = await api("/companies/" + companyId + "/jobs/" + jobId);
    onStatus(job.status);
    if (job.status === "done") return job.result;
    if (job.status === "failed") throw new Error(job.error || "jobben feilet");
  }
}
