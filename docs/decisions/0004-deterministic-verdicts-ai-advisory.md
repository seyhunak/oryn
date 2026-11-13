# 0004: Deterministic verdicts; AI is advisory and labelled

- Status: accepted
- Date: 2026-11-13 (recorded retrospectively from the code)

## Context and problem statement

Language models can draft tests and explain failures, but they are non-deterministic, can be manipulated through their input, and can be confidently wrong. A quality platform whose verdicts depend on a model cannot be trusted as evidence. What may AI decide, and how is its output kept apart from facts?

## Decision drivers

- Verdicts must be reproducible from recorded evidence.
- Untrusted content (repositories, requirements, page text) reaches the model.
- The product must work without any AI provider.

## Considered options

1. Verdicts only from deterministic assertions; AI output is a proposal or an interpretation, stored and labelled separately, and every proposal needs human approval.
2. Let the model judge outcomes where assertions are hard to write ("does the page look right?").
3. Let AI-generated scenarios run without approval.

## Decision outcome

Option 1:

- Job and execution status are set only from step results computed by the API engine or the Playwright runner. No code path writes a status from model output.
- Test plans (heuristic or AI) are `draft` until someone with `test:approve` approves them; approval creates suites whose scenarios are `approved`. Workspaces with `require_approval = true` (default) cannot run draft scenarios.
- Failure investigation runs deterministic rules first. An optional AI interpretation is stored in `failure_investigations.interpretation`, labelled "AI interpretation", and `deterministic_category` always keeps the rule result. The AI category is used only when the rules returned `unknown`.
- Automatic investigation after a failed job never calls a model.
- Regression selection and recommendations are deterministic and explain their scores.
- Every feature has a non-AI path: heuristic planning, rule-based investigation.

### Consequences

- Good: a verdict can always be traced to an assertion with expected and actual values.
- Good: prompt injection can at worst produce a bad proposal, which a person reviews.
- Bad: some checks that a model could approximate (visual correctness, free-text judgement) are not offered.
- Bad: approval adds a step before AI-planned tests run.

## Pros and cons of the other options

- **Model-judged outcomes**: broader coverage, but verdicts become non-reproducible and attackable through page content.
- **Unapproved AI scenarios**: faster, but lets unreviewed and possibly injected steps run against real environments with real secrets.
