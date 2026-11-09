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
  let wait = 300;
  for (;;) {
    await new Promise((r) => setTimeout(r, wait));
    wait = Math.min(wait * 1.5, 3000);
    const job = await api("/companies/" + companyId + "/jobs/" + started.job_id);
    onStatus(job.status);
    if (job.status === "done") return job.result;
    if (job.status === "failed") throw new Error(job.error || "jobben feilet");
  }
}
