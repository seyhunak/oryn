# Security policy

## Supported versions

ORYN is pre-1.0. Security fixes land on `main` and in the next release; there are no
long-term support branches yet.

## Reporting a vulnerability

Please do **not** open a public issue. Use GitHub's
[private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)
on this repository ("Security" → "Report a vulnerability").

Include what you can: affected component (API, worker, runner, web), version or commit,
a description, reproduction steps or a proof of concept, and the impact you expect.

We aim to acknowledge reports within three working days, agree a fix and disclosure date
with you, and credit you in the advisory unless you prefer otherwise.

## Scope

In scope: the ORYN control plane, worker, Playwright runner, web UI, Docker images and
deployment manifests in this repository. Out of scope: the Northwind demo application
(`examples/demo-application`, which contains deliberate defects), and vulnerabilities in
third-party services you connect (model providers, Git hosts).

How ORYN is secured, and its known limitations, are described in
[docs/SECURITY.md](docs/SECURITY.md).
