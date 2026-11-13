# 0005: Model gateway with structured outputs and a provider abstraction

- Status: accepted
- Date: 2026-11-13 (recorded retrospectively from the code)

## Context and problem statement

Self-hosted users bring different model providers: hosted APIs, Azure tenants, local models. AI output feeds code paths (plans become Gherkin, investigations become records), so free text is not usable. Prompts carry untrusted content and possibly secrets. How should ORYN call models?

## Decision drivers

- Provider choice per workspace, including local models.
- Machine-checkable output.
- Redaction and injection hardening in one place.
- Cost and usage accounting.
- Tests that run without credentials.

## Considered options

1. A Rust gateway crate with one trait, provider adapters, JSON-schema-constrained output, and central redaction, fencing, retries and accounting.
2. A third-party SDK or proxy (for example an OpenAI-compatible router) in front of every provider.
3. Free-text prompts parsed with regular expressions.

## Decision outcome

Option 1, [`crates/oryn-model-gateway`](../../crates/oryn-model-gateway):

- Providers: `anthropic`, `openai`, `azure_openai`, `gemini`, `ollama`, `openai_compatible`, and a deterministic `mock` used by tests and the demo seed.
- Each request names a JSON schema. The adapter uses the provider's structured-output mechanism (Anthropic `output_config.format`, OpenAI `response_format`, Gemini response schema, Ollama `format`). The response is validated; one repair attempt is made, then the call fails.
- All prompt text is redacted. Untrusted inputs are wrapped by `untrusted()` in a fence, and a fixed safety preamble tells the model to treat fenced text as data.
- Up to 3 attempts with exponential back-off, honouring `retry-after` up to 30 s.
- The API records every call in `ai_requests` (purpose, model, tokens, cost from per-model prices, latency, status) and aggregates `ai_usage`.
- Providers and models are configured per workspace; keys are encrypted with `SecretBox`.

### Consequences

- Good: callers receive typed, validated JSON; tests run with the mock provider and no network.
- Good: adding a provider is one adapter.
- Bad: providers without structured outputs, or local models that follow schemas poorly, fail more often and cost a repair call.
- Bad: no streaming; long generations wait for the full response.
- Neutral: the gateway has no tool-calling support by design (see [0004](0004-deterministic-verdicts-ai-advisory.md)).

## Pros and cons of the other options

- **Third-party proxy**: fewer adapters to maintain, but another service to deploy and less control over redaction and schemas.
- **Free text**: works with any model but cannot safely feed compilation or storage.
