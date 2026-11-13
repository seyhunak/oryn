#!/usr/bin/env node
// Verify a demo run end to end: wait for every execution to finish, then check that
// ORYN detected exactly the planted defects (examples/demo-application/README.md) and
// that every other scenario passed. Used by CI after `seed.mjs --run`.
//
// Exit code 0 = the platform behaved as specified; 1 = something else failed or a
// planted defect went undetected.

const API = (process.env.ORYN_URL ?? "http://localhost:8080").replace(/\/$/, "");
const EMAIL = process.env.ORYN_SEED_EMAIL ?? "admin@oryn.local";
const PASSWORD = process.env.ORYN_SEED_PASSWORD ?? "oryn-demo-password";
const TIMEOUT_MS = Number(process.env.VERIFY_TIMEOUT_MS ?? 600_000);

// Scenarios that MUST fail (deterministic planted defects 1 and 3).
const MUST_FAIL = new Map([
  ["Buy two of a cheap item", "defect 1: discount ignores quantity"],
  ["Two of a cheap item are priced per the spec", "defect 1: discount ignores quantity"],
  ["Request a reset link from the login page", "defect 3: reset confirmation text"],
  ["Unknown email gets the same confirmation", "defect 3: reset confirmation text"],
]);
// Scenarios that MAY fail (defect 2: intermittent 503 from search, mitigated by retries).
const MAY_FAIL = new Set(["Search the catalogue"]);

let cookie = "";
async function call(method, url, body) {
  const res = await fetch(`${API}${url}`, {
    method,
    headers: { "content-type": "application/json", "x-oryn-csrf": "1", ...(cookie ? { cookie } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const set = res.headers.get("set-cookie");
  if (set) cookie = set.split(";")[0];
  const json = await res.json().catch(() => null);
  if (!res.ok) throw new Error(`${method} ${url} → ${res.status}: ${json?.error?.message ?? ""}`);
  return json;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  await call("POST", "/api/v1/auth/login", { email: EMAIL, password: PASSWORD });
  const me = await call("GET", "/api/v1/auth/me");
  const ws = me.workspaces[0].id;
  const deadline = Date.now() + TIMEOUT_MS;
  let runs;
  for (;;) {
    runs = (await call("GET", `/api/v1/workspaces/${ws}/executions?per_page=100`)).items;
    const active = runs.filter((e) => e.status === "queued" || e.status === "running");
    if (!active.length) break;
    if (Date.now() > deadline) throw new Error(`${active.length} executions still active after ${TIMEOUT_MS} ms`);
    await sleep(3000);
  }
  // Latest execution per suite.
  const latest = new Map();
  for (const e of runs) if (e.suite_id && !latest.has(e.suite_id)) latest.set(e.suite_id, e);
  if (!latest.size) throw new Error("no executions found; run `node scripts/seed.mjs --run` first");

  const problems = [];
  const detected = new Set();
  let passed = 0;
  for (const e of latest.values()) {
    const d = await call("GET", `/api/v1/executions/${e.id}`);
    for (const j of d.jobs.filter((x) => !x.superseded)) {
      const base = j.scenario_name.split(" (")[0];
      const failed = j.status !== "passed";
      if (MUST_FAIL.has(base)) {
        if (failed) detected.add(base);
        else problems.push(`planted defect not detected: "${j.scenario_name}" passed (${MUST_FAIL.get(base)})`);
      } else if (failed && !MAY_FAIL.has(base)) {
        problems.push(`unexpected ${j.status}: "${j.scenario_name}" in ${e.title}: ${j.error ?? "see /executions/" + e.id}`);
      } else if (!failed) passed++;
    }
  }
  for (const name of MUST_FAIL.keys()) if (!detected.has(name) && !problems.some((p) => p.includes(name))) problems.push(`planted-defect scenario missing from the run: "${name}"`);

  console.log(`${latest.size} suites, ${passed} scenarios passed, ${detected.size}/${MUST_FAIL.size} planted-defect scenarios failed as expected.`);
  if (problems.length) {
    for (const p of problems) console.error("✗", p);
    process.exit(1);
  }
  console.log("✓ demo verified");
}

main().catch((e) => {
  console.error("verify failed:", e.message);
  process.exit(1);
});
