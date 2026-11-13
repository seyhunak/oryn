#!/usr/bin/env node
// Seed a running ORYN instance with the Northwind Supply demo project.
//
// Idempotent: re-running reuses the account, workspace and project and only adds what
// is missing. Talks to the public HTTP API exactly like the UI does; it never touches
// the database directly.
//
//   ORYN_URL            API base (default http://localhost:8080)
//   ORYN_SEED_EMAIL     admin email (default admin@oryn.local)
//   ORYN_SEED_PASSWORD  admin password (default oryn-demo-password)
//   DEMO_BASE_URL       where the demo app is reachable *from workers* (default http://localhost:4000)
//   DEMO_REPO_PATH      directory under ORYN_LOCAL_IMPORT_ROOT holding the demo source (default demo-application)
//   ORYN_SEED_AI        "mock" to register the deterministic mock model provider (default mock)
//
// Pass --run to also start one execution per suite (needs a worker; browser suites need a
// Playwright runner attached to that worker).

import { readFile, readdir } from "node:fs/promises";
import path from "node:path";

const API = (process.env.ORYN_URL ?? "http://localhost:8080").replace(/\/$/, "");
const EMAIL = process.env.ORYN_SEED_EMAIL ?? "admin@oryn.local";
const PASSWORD = process.env.ORYN_SEED_PASSWORD ?? "oryn-demo-password";
const DEMO_BASE = process.env.DEMO_BASE_URL ?? "http://localhost:4000";
const DEMO_REPO = process.env.DEMO_REPO_PATH ?? "demo-application";
const AI = process.env.ORYN_SEED_AI ?? "mock";
const DEMO_DIR = path.resolve(import.meta.dirname, "../examples/demo-application");

let cookie = "";

async function call(method, url, body) {
  const res = await fetch(`${API}${url}`, {
    method,
    headers: { "content-type": "application/json", "x-oryn-csrf": "1", ...(cookie ? { cookie } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const set = res.headers.get("set-cookie");
  if (set) cookie = set.split(";")[0];
  const text = await res.text();
  const json = text ? JSON.parse(text) : null;
  if (!res.ok) {
    const err = new Error(`${method} ${url} → ${res.status}: ${json?.error?.message ?? text}`);
    err.status = res.status;
    throw err;
  }
  return json;
}

const log = (...a) => console.log("·", ...a);

async function main() {
  await call("GET", "/healthz");
  const providers = await call("GET", "/api/v1/auth/providers");
  let me;
  if (providers.needs_setup) {
    me = await call("POST", "/api/v1/auth/register", { email: EMAIL, name: "Demo Admin", password: PASSWORD, workspace_name: "Northwind QA" });
    log("created admin", EMAIL);
  } else {
    await call("POST", "/api/v1/auth/login", { email: EMAIL, password: PASSWORD });
    me = await call("GET", "/api/v1/auth/me");
    log("signed in as", EMAIL);
  }
  const ws = me.workspaces[0].id;

  const projects = await call("GET", `/api/v1/workspaces/${ws}/projects`);
  let project = projects.find((p) => p.slug === "northwind-supply");
  if (!project) {
    project = await call("POST", `/api/v1/workspaces/${ws}/projects`, {
      name: "Northwind Supply",
      description: "Hardware-store demo with planted defects: a quantity-blind discount, a flaky search backend and a wrong reset message.",
      visibility: "workspace",
    });
    log("created project", project.name);
  }
  const pid = project.id;

  const envs = await call("GET", `/api/v1/projects/${pid}/environments`);
  if (!envs.some((e) => e.name === "local")) {
    await call("POST", `/api/v1/projects/${pid}/environments`, {
      name: "local",
      base_url: DEMO_BASE,
      api_base_url: DEMO_BASE,
      is_default: true,
      variables: { email: "demo@northwind.test" },
      secrets: { password: "demo-password" },
      allowed_hosts: [],
    });
    log("created environment local →", DEMO_BASE);
  }

  const reqs = await call("GET", `/api/v1/projects/${pid}/requirements`);
  if (reqs.length === 0) {
    const content = await readFile(path.join(DEMO_DIR, "docs/requirements.md"), "utf8");
    const r = await call("POST", `/api/v1/projects/${pid}/requirements/import`, { content, format: "markdown" });
    log("imported", r.created.length, "requirements");
  }

  const repos = await call("GET", `/api/v1/projects/${pid}/repositories`);
  if (repos.length === 0) {
    try {
      await call("POST", `/api/v1/projects/${pid}/repositories`, { source: "local", path: DEMO_REPO });
      log("importing repository from", DEMO_REPO);
    } catch (e) {
      log("repository import skipped:", e.message);
    }
  }

  const suites = await call("GET", `/api/v1/projects/${pid}/suites`);
  const files = (await readdir(path.join(DEMO_DIR, "features"))).filter((f) => f.endsWith(".feature")).sort();
  for (const f of files) {
    const rel = `features/${f}`;
    if (suites.some((s) => s.path === rel)) continue;
    const source = await readFile(path.join(DEMO_DIR, rel), "utf8");
    const saved = await call("POST", `/api/v1/projects/${pid}/suites`, { source, path: rel });
    const ok = saved.validation?.ok ? "valid" : `${saved.validation?.diagnostics?.length ?? 0} diagnostics`;
    log("suite", rel, ok);
  }

  const scenarios = await call("GET", `/api/v1/projects/${pid}/scenarios`);
  const drafts = scenarios.filter((s) => s.status === "draft" && s.executable !== false).map((s) => s.id);
  if (drafts.length) {
    const r = await call("POST", `/api/v1/projects/${pid}/scenarios/status`, { scenario_ids: drafts, status: "approved", comment: "Seeded demo suite" });
    log("approved", r.updated ?? drafts.length, "scenarios");
  }

  if (AI === "mock") {
    const ai = await call("GET", `/api/v1/workspaces/${ws}/ai/providers`);
    if (!ai.some((p) => p.kind === "mock")) {
      await call("POST", `/api/v1/workspaces/${ws}/ai/providers`, { kind: "mock", name: "Mock (deterministic)", is_default: true });
      log("registered the mock model provider");
    }
  }

  if (process.argv.includes("--run")) {
    const all = await call("GET", `/api/v1/projects/${pid}/suites`);
    for (const suite of all) {
      const e = await call("POST", `/api/v1/projects/${pid}/executions`, { suite_id: suite.id, trigger: "manual", retry_on_failure: true, max_attempts: 2 });
      log("started", suite.name, "→", `/executions/${e.id}`);
    }
  }

  console.log(`\nSeeded. Sign in at the web UI as ${EMAIL} / ${PASSWORD}.`);
  console.log("Start a worker and the demo app, then run a suite from Test Studio.");
}

main().catch((e) => {
  console.error("seed failed:", e.message);
  process.exit(1);
});
