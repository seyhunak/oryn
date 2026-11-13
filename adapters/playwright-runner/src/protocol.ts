// oryn.browser/v1 wire types and request validation.
// The normative description lives in docs/protocol/browser-v1.md; keep both in sync.

import { z } from 'zod';

export const PROTOCOL = 'oryn.browser/v1';

export const BROWSER_NAMES = ['chromium', 'firefox', 'webkit'] as const;
export type BrowserName = (typeof BROWSER_NAMES)[number];

export const MAX_WAIT_STEP_MS = 30_000;
export const MAX_STEP_TIMEOUT_MS = 120_000;
export const MAX_STEPS = 500;

const id = z.string().min(1).max(128);
const target = z.string().min(1).max(2048);
const waitMs = z.number().int().min(0).max(MAX_STEP_TIMEOUT_MS).optional();

const step = <A extends string, S extends z.ZodRawShape>(action: A, shape: S) =>
  z.object({ id, action: z.literal(action), ...shape }).strict();

export const stepSchema = z.discriminatedUnion('action', [
  step('goto', { url: z.string().min(1).max(4096) }),
  step('click', { target }),
  step('fill', { target, value: z.string().max(100_000) }),
  step('press', { key: z.string().min(1).max(64), target: target.optional() }),
  step('select', { target, value: z.string().max(4096) }),
  step('check', { target }),
  step('uncheck', { target }),
  step('hover', { target }),
  step('wait', { ms: z.number().int().min(0) }),
  step('screenshot', { name: z.string().min(1).max(128) }),
  step('observe_text', { text: z.string().min(1).max(4096), visible: z.boolean().optional(), wait_ms: waitMs }),
  step('observe_element', { target, contains: z.string().max(4096).optional(), wait_ms: waitMs }),
  step('observe_url', { contains: z.string().max(4096).optional(), wait_ms: waitMs }),
  step('observe_title', { equals: z.string().max(4096).optional(), wait_ms: waitMs }),
]);

export type Step = z.infer<typeof stepSchema>;
export type StepAction = Step['action'];

export const OBSERVATION_ACTIONS: ReadonlySet<StepAction> = new Set([
  'observe_text',
  'observe_element',
  'observe_url',
  'observe_title',
]);

const cookieSchema = z
  .object({
    name: z.string(),
    value: z.string(),
    domain: z.string(),
    path: z.string(),
    expires: z.number(),
    httpOnly: z.boolean(),
    secure: z.boolean(),
    sameSite: z.enum(['Strict', 'Lax', 'None']),
  })
  .strict();

export const storageStateSchema = z
  .object({
    cookies: z.array(cookieSchema),
    origins: z.array(
      z
        .object({
          origin: z.string(),
          localStorage: z.array(z.object({ name: z.string(), value: z.string() }).strict()),
        })
        .strict(),
    ),
  })
  .strict();

const httpUrl = z
  .string()
  .url()
  .refine((u) => /^https?:\/\//i.test(u), { message: 'base_url must be an http(s) URL' });

export const runRequestSchema = z
  .object({
    protocol: z.literal(PROTOCOL),
    run_id: z.string().min(1).max(128),
    browser: z.enum(BROWSER_NAMES).default('chromium'),
    base_url: httpUrl,
    viewport: z
      .object({ width: z.number().int().min(200).max(7680), height: z.number().int().min(200).max(4320) })
      .strict()
      .default({ width: 1280, height: 800 }),
    step_timeout_ms: z.number().int().min(100).max(MAX_STEP_TIMEOUT_MS).default(15_000),
    screenshots: z.enum(['off', 'on_failure', 'every_step']).default('on_failure'),
    record_video: z.boolean().default(false),
    record_trace: z.boolean().default(false),
    headers: z.record(z.string(), z.string()).default({}),
    storage_state: storageStateSchema.nullable().default(null),
    allowed_hosts: z.array(z.string().min(1).max(255)).max(256).default([]),
    steps: z.array(stepSchema).min(1).max(MAX_STEPS),
  })
  .strict()
  .superRefine((req, ctx) => {
    const seen = new Set<string>();
    req.steps.forEach((s, i) => {
      if (seen.has(s.id)) {
        ctx.addIssue({ code: 'custom', path: ['steps', i, 'id'], message: `duplicate step id "${s.id}"` });
      }
      seen.add(s.id);
    });
  });

export type RunRequest = z.infer<typeof runRequestSchema>;

export type ValidationResult =
  | { ok: true; value: RunRequest }
  | { ok: false; code: 'invalid_request' | 'unsupported_protocol'; message: string };

/** Validate a parsed JSON body. Protocol mismatches are reported separately from shape errors. */
export function validateRunRequest(body: unknown): ValidationResult {
  if (typeof body !== 'object' || body === null || Array.isArray(body)) {
    return { ok: false, code: 'invalid_request', message: 'request body must be a JSON object' };
  }
  const protocol = (body as Record<string, unknown>)['protocol'];
  if (protocol === undefined) {
    return { ok: false, code: 'invalid_request', message: `protocol: required (expected "${PROTOCOL}")` };
  }
  if (protocol !== PROTOCOL) {
    return {
      ok: false,
      code: 'unsupported_protocol',
      message: `unsupported protocol ${JSON.stringify(protocol)}; this runner speaks "${PROTOCOL}"`,
    };
  }
  const parsed = runRequestSchema.safeParse(body);
  if (!parsed.success) return { ok: false, code: 'invalid_request', message: formatIssues(parsed.error) };
  return { ok: true, value: parsed.data };
}

export function formatIssues(error: z.ZodError): string {
  return error.issues
    .slice(0, 10)
    .map((i) => `${i.path.length ? i.path.join('.') : '(root)'}: ${i.message}`)
    .join('; ');
}

export const recordingRequestSchema = z
  .object({
    url: httpUrl,
    browser: z.enum(BROWSER_NAMES).default('chromium'),
  })
  .strict();

// ------------------------------------------------------------------ responses

export type ErrorKind = 'timeout' | 'not_found' | 'navigation' | 'blocked' | 'unknown';

export interface StepError {
  kind: ErrorKind;
  message: string;
}

export type Observation =
  | { visible: boolean; count: number }
  | { visible: boolean; count: number; text: string | null; value: string | null }
  | { url: string; path: string }
  | { title: string };

export interface StepResult {
  id: string;
  action: StepAction;
  status: 'ok' | 'error' | 'skipped';
  started_at: string | null;
  duration_ms: number;
  observation: Observation | null;
  error: StepError | null;
  screenshot: string | null;
}

export interface Artifact {
  name: string;
  kind: 'screenshot' | 'video' | 'trace';
  content_type: string;
  data_base64: string;
  size_bytes: number;
}

export interface ConsoleEntry {
  type: string;
  text: string;
  timestamp: string;
}

export interface NetworkEntry {
  url: string;
  method: string;
  status: number | null;
  failure: string | null;
}

export interface RunResponse {
  protocol: typeof PROTOCOL;
  run_id: string;
  status: 'completed' | 'aborted';
  browser: BrowserName;
  browser_version: string;
  started_at: string;
  duration_ms: number;
  steps: StepResult[];
  artifacts: Artifact[];
  console: ConsoleEntry[];
  network: NetworkEntry[];
}

export interface ApiErrorBody {
  error: { code: string; message: string };
}
