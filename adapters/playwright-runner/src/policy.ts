// Network policy (pure, unit tested).

/**
 * Hosts that are always blocked regardless of allowed_hosts (cloud metadata endpoints).
 * Keep in sync with METADATA_HOSTS in crates/oryn-api-engine/src/policy.rs.
 */
export const ALWAYS_BLOCKED_HOSTS: ReadonlySet<string> = new Set([
  '169.254.169.254',
  'metadata.google.internal',
  'metadata',
  '100.100.100.200',
  'fd00:ec2::254',
]);

export type PolicyDecision = 'allow' | 'block';

/**
 * Decide whether a request URL may proceed.
 * - `data:` and `blob:` URLs are always allowed.
 * - Cloud metadata hosts are always blocked.
 * - If `allowedHosts` is empty, everything else is allowed.
 * - Otherwise the URL's hostname must equal one of the entries, or match a
 *   `*.domain` entry (the domain itself or any subdomain). Case-insensitive;
 *   IPv6 literals may be given with or without brackets. Same rules as the Rust policy.
 */
export function networkDecision(rawUrl: string, allowedHosts: readonly string[]): PolicyDecision {
  let url: URL;
  try {
    url = new URL(rawUrl);
  } catch {
    return allowedHosts.length === 0 ? 'allow' : 'block';
  }
  if (url.protocol === 'data:' || url.protocol === 'blob:') return 'allow';
  const host = url.hostname.toLowerCase().replace(/^\[|\]$/g, '');
  if (ALWAYS_BLOCKED_HOSTS.has(host)) return 'block';
  if (allowedHosts.length === 0) return 'allow';
  const allowed = allowedHosts.map((h) => h.toLowerCase().replace(/^\[|\]$/g, ''));
  const ok = allowed.some((a) =>
    a.startsWith('*.') ? host === a.slice(2) || host.endsWith(a.slice(1)) : a === host,
  );
  return ok ? 'allow' : 'block';
}
