// Status words and tones (docs/ui-guidelines.md §4): the one place server states become words.

export type Tone = "ok" | "busy" | "bad" | "off";

/** A Content API key: usable until it expires (revoked keys are not listed). */
export function keyStatus(expiresAt: string | null, now = Date.now()): [Tone, string] {
  const expired = expiresAt != null && new Date(expiresAt).getTime() <= now;
  return expired ? ["off", "Expired"] : ["ok", "Active"];
}
