import createClient, { type Middleware } from "openapi-fetch";
import type { paths, components } from "./schema";

export type Schemas = components["schemas"];

/** Error shape every API failure returns: `{ error: { code, message } }`. */
export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
  }
}

const csrf: Middleware = {
  onRequest({ request }) {
    // Cookie-authenticated writes must carry this header (the API refuses them otherwise).
    request.headers.set("x-oryn-csrf", "1");
    return request;
  },
};

export const api = createClient<paths>({ baseUrl: "", credentials: "same-origin" });
api.use(csrf);

type Result<T> = { data?: T; error?: unknown; response: Response };

/** Return the data or throw an ApiError with the server's message. */
export async function unwrap<T>(p: Promise<Result<T>>): Promise<T> {
  const { data, error, response } = await p;
  if (!response.ok) {
    const e = (error as { error?: { code?: string; message?: string } } | undefined)?.error;
    throw new ApiError(response.status, e?.code ?? "http_error", e?.message ?? `${response.status} ${response.statusText}`);
  }
  return data as T;
}

/** Raw upload (ZIP archives): openapi-fetch serialises JSON, so binary bodies go through fetch. */
export async function uploadRaw<T>(url: string, body: Blob): Promise<T> {
  const res = await fetch(url, { method: "POST", body, credentials: "same-origin", headers: { "x-oryn-csrf": "1", "content-type": "application/zip" } });
  const json = await res.json().catch(() => null);
  if (!res.ok) throw new ApiError(res.status, json?.error?.code ?? "http_error", json?.error?.message ?? res.statusText);
  return json as T;
}

export function errorMessage(e: unknown): string {
  if (e instanceof ApiError) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}
