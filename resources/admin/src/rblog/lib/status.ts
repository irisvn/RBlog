// Status words and tones (docs/ui-guidelines.md §4): the one place server states become words.

export type Tone = "ok" | "busy" | "bad" | "off";

/** A post or page in the publishing workflow (`models::post::PostStatus`). An unknown value is
 * shown as itself, `off`. */
export function contentStatus(value: unknown): [Tone, string] {
  switch (value) {
    case "published":
      return ["ok", "Published"];
    case "pending":
      return ["busy", "Pending review"];
    case "scheduled":
      return ["busy", "Scheduled"];
    case "draft":
      return ["off", "Draft"];
    default:
      return ["off", typeof value === "string" ? value : ""];
  }
}

/** A Content API key: usable until it expires (revoked keys are not listed). */
export function keyStatus(expiresAt: string | null, now = Date.now()): [Tone, string] {
  const expired = expiresAt != null && new Date(expiresAt).getTime() <= now;
  return expired ? ["off", "Expired"] : ["ok", "Active"];
}

/** Where the running build stands against the releases (`services::version::state`). */
export function versionStatus(state: string): [Tone, string] {
  switch (state) {
    case "update":
      return ["busy", "Update available"];
    case "current":
      return ["ok", "Up to date"];
    case "no_release":
      return ["off", "No release yet"];
    case "not_checked":
      return ["off", "Not checked"];
    case "not_compared":
      return ["off", "Not compared"];
    default:
      return ["off", state];
  }
}
