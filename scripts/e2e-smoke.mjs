// End-to-end smoke test of the publishing path, against a running panel with the seeded roles
// (`rblog db:seed`) and, ideally, the demo data. Not run in CI. Uses the installed Google Chrome
// through playwright-core (nothing is downloaded): run `npm ci` in resources/admin first.
//
//   RUSTCMS_WEBHOOK__ALLOW_INSECURE_LOCAL=true rblog serve          (in another terminal)
//   RBLOG_E2E_ADMIN_PASSWORD=… node scripts/e2e-smoke.mjs
//
// What it does, each step stopping the run when it fails:
//   1. as a super admin: makes (or reuses, with a new random password: that needs a super admin
//      of every guard, otherwise it makes a new pair for the run) an Editor and a Contributor
//      through the Users API (RbacService, the seeded roles); points General → Public site URL and
//      Integrations → Webhook URL/secret at a tiny local server this script starts; issues a
//      Content API key;
//   2. the Contributor writes a draft, gets a preview link from the preview slot, and opens it:
//      the local "frontend" asks RBlog's /api/v1/preview for it, as a real one would;
//   3. the Contributor submits it for review, and sees no Publish action (and the server refuses it);
//   4. the Editor publishes it from the list (selection bar);
//   5. the Content API returns it with the key;
//   6. the local receiver gets a `post.published` webhook whose signature checks out
//      (docs/frontend-integration.html, "Webhook").
// Afterwards (also after a failure): the key is revoked, the post trashed, the settings put back
// and both users deactivated.
//
// Environment (all optional but the password, given directly or in RBLOG_E2E_ADMIN_PASSWORD_FILE):
//   RBLOG_E2E_BASE            the app (http://localhost:8000); the panel is at /admin
//   RBLOG_E2E_ADMIN_EMAIL     a super admin (shot@example.com)
//   RBLOG_E2E_PORT            the local frontend/receiver port (a free one)
//
// The webhook URL must be unset before the run (the script refuses otherwise: it cannot put
// back a secret it never saw), and the server must allow http://localhost receivers
// (`[webhook] allow_insecure_local`, development only).
import { createRequire } from "node:module";
import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import fs from "node:fs";
import http from "node:http";

const require = createRequire(new URL("../resources/admin/package.json", import.meta.url));
const { chromium } = require("playwright-core");

const env = (name, fallback) => process.env[name]?.trim() || fallback;
const base = env("RBLOG_E2E_BASE", "http://localhost:8000").replace(/\/$/, "");
const admin = `${base}/admin`;
const adminEmail = env("RBLOG_E2E_ADMIN_EMAIL", "shot@example.com");
const passwordFile = process.env.RBLOG_E2E_ADMIN_PASSWORD_FILE;
const adminPassword = process.env.RBLOG_E2E_ADMIN_PASSWORD || (passwordFile && fs.readFileSync(passwordFile, "utf8").trim());
if (!adminPassword) {
  console.error("RBLOG_E2E_ADMIN_PASSWORD (or RBLOG_E2E_ADMIN_PASSWORD_FILE) is required");
  process.exit(2);
}

const random = (bytes = 18) => randomBytes(bytes).toString("base64url");
const run = random(4).toLowerCase().replace(/[^a-z0-9]/g, "x");
const USERS = {
  editor: { email: "e2e-editor@example.com", name: "E2E Editor", role: "Editor", password: random() },
  contributor: { email: "e2e-contributor@example.com", name: "E2E Contributor", role: "Contributor", password: random() },
};
const TITLE = `E2E smoke ${run}`;
const SECRET = random(32);

// ---- The local frontend: the webhook receiver and the preview route -------------------------

const webhooks = [];
const previews = [];
// A failure here answers 500 (the step waiting for it fails, and the cleanup runs) instead of an
// unhandled rejection that would end the process before the cleanup.
const server = http.createServer((req, res) => {
  frontendRoute(req, res).catch((err) => {
    previews.push({ status: 0, error: String(err?.message ?? err) });
    if (!res.headersSent) res.writeHead(500);
    res.end();
  });
});

async function frontendRoute(req, res) {
  const url = new URL(req.url, "http://localhost");
  if (req.method === "POST" && url.pathname === "/webhook") {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    webhooks.push({ headers: req.headers, body: Buffer.concat(chunks) });
    res.writeHead(204).end();
    return;
  }
  if (req.method === "GET" && url.pathname === "/preview") {
    // As the frontend's preview route does: from the server, without a key, never cached.
    const token = url.searchParams.get("token") ?? "";
    const answer = await fetch(`${base}/api/v1/preview?token=${encodeURIComponent(token)}`, { headers: { Accept: "application/json" } });
    const json = answer.ok ? await answer.json() : null;
    previews.push({ status: answer.status, json });
    res.writeHead(answer.ok ? 200 : 502, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "private, no-store" });
    res.end(json ? `<h1>${escapeHtml(json.title)}</h1><p class="status">${escapeHtml(json.preview?.status)}</p>` : `<p>RBlog preview ${answer.status}</p>`);
    return;
  }
  res.writeHead(404).end();
}
await new Promise((resolve) => server.listen(Number(env("RBLOG_E2E_PORT", "0")), "127.0.0.1", resolve));
const frontend = `http://localhost:${server.address().port}`;

function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

/** The receiver's check (docs/frontend-integration.html): HMAC-SHA256 hex of `ts.id.body`,
 * compared in constant time, and a timestamp at most 5 minutes off. */
function verify(secret, headers, body) {
  const id = headers["x-rblog-event-id"] ?? "";
  const ts = headers["x-rblog-timestamp"] ?? "";
  const sig = headers["x-rblog-signature"] ?? "";
  const expected = createHmac("sha256", secret).update(`${ts}.${id}.`).update(body).digest();
  const given = Buffer.from(sig, "hex");
  if (given.length !== expected.length || !timingSafeEqual(given, expected)) return "bad signature";
  if (Math.abs(Date.now() / 1000 - Number(ts)) > 300) return "stale timestamp";
  return null;
}

// ---- Steps and cleanup ------------------------------------------------------------------------

const cleanups = []; // [what, fn], undone last first
let failed = false;
async function step(name, fn) {
  const started = Date.now();
  try {
    const note = await fn();
    console.log(`ok   ${name}${note ? `: ${note}` : ""} (${Date.now() - started} ms)`);
  } catch (err) {
    console.log(`FAIL ${name}: ${String(err?.message ?? err).split("\n")[0]}`);
    throw err;
  }
}
function check(condition, message) {
  if (!condition) throw new Error(message);
}
const until = async (what, fn, ms = 60000) => {
  const deadline = Date.now() + ms;
  for (;;) {
    const value = await fn();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 500));
  }
};

const browser = await chromium.launch({ channel: "chrome" });

/** A signed-in page (English) for `email`. */
async function signIn(email, password) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, locale: "en" });
  await context.addInitScript(() => {
    try {
      localStorage.setItem("rustcms-admin-locale", "en");
    } catch {}
  });
  const page = await context.newPage();
  await page.goto(`${admin}/login`);
  await page.locator("input[type=email]").fill(email);
  await page.locator("input[type=password]").fill(password);
  await page.locator("button[type=submit]").click();
  await page.waitForURL((url) => !url.pathname.endsWith("/login"), { timeout: 20000 });
  return page;
}

/** A panel API call through `page`'s session: `{status, body}`. */
async function api(page, method, path, body) {
  return page.evaluate(
    async ([prefix, method, path, body]) => {
      const headers = { Accept: "application/json" };
      if (method !== "GET") {
        headers["X-CSRF-Token"] = (await fetch(`${prefix}/api/auth/csrf`).then((r) => r.json())).csrfToken;
        headers["Content-Type"] = "application/json";
      }
      const response = await fetch(`${prefix}/api${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
      const text = await response.text();
      let json = null;
      try {
        json = text ? JSON.parse(text) : null;
      } catch {}
      return { status: response.status, body: json ?? text };
    },
    [new URL(admin).pathname, method, path, body],
  );
}
const expect = (response, status, what) => {
  check(response.status === status, `${what}: HTTP ${response.status} ${JSON.stringify(response.body).slice(0, 300)}`);
  return response.body;
};

/** Saves `data` in the settings group `group` over its current version. */
async function saveSettings(page, group, data) {
  const current = expect(await api(page, "GET", `/settings/${group}`), 200, `reading ${group} settings`);
  return expect(await api(page, "PUT", `/settings/${group}`, { version: current.version, data }), 200, `saving ${group} settings`);
}

/** Runs `click` and waits for the answer of the post action `name`, which must succeed. */
async function runAction(page, name, click) {
  const answered = page.waitForResponse((r) => r.request().method() === "POST" && new URL(r.url()).pathname.endsWith(`/resources/posts/actions/${name}`));
  await click();
  const response = await answered;
  check(response.ok(), `the ${name} action: HTTP ${response.status()} ${(await response.text()).slice(0, 200)}`);
}

/** The row of the post in the list (searched by its title). */
async function postRow(page) {
  await page.goto(`${admin}/r/posts?search=${encodeURIComponent(TITLE)}`);
  const row = page.getByRole("row").filter({ hasText: TITLE }).first();
  await row.waitFor({ timeout: 15000 });
  return row;
}

let superAdmin;
let postId;
let key;
const accounts = {};

try {
  await step("sign in as the super admin", async () => {
    superAdmin = await signIn(adminEmail, adminPassword);
    return adminEmail;
  });

  await step("make the Editor and the Contributor (RbacService, seeded roles)", async () => {
    const roles = expect(await api(superAdmin, "GET", "/system/roles"), 200, "listing roles");
    cleanups.push(["deactivate the users", async () => {
      for (const [kind, id] of Object.entries(accounts)) {
        const r = await api(superAdmin, "PUT", `/system/users/${id}/active`, { active: false, expected: true });
        check(r.status === 200, `deactivating the ${kind}: HTTP ${r.status}`);
      }
    }]);
    for (const [kind, user] of Object.entries(USERS)) {
      const role = roles.find((r) => r.name === user.role && (!r.guard || r.guard === "admin"));
      check(role, `no ${user.role} role: run \`rblog db:seed\``);
      const found = expect(await api(superAdmin, "GET", `/system/users?search=${encodeURIComponent(user.email)}`), 200, "searching users");
      const existing = found.data.find((u) => u.email === user.email);
      if (existing) {
        // Reused with a new password. Setting someone's password needs a super admin of every
        // guard (RbacService::reset_password); without that, this run makes its own users.
        const reset = await api(superAdmin, "PUT", `/system/users/${existing.id}/password`, { password: user.password });
        if (reset.status === 204) {
          accounts[kind] = existing.id;
          if (!existing.isActive) {
            expect(await api(superAdmin, "PUT", `/system/users/${existing.id}/active`, { active: true, expected: false }), 200, `activating ${user.email}`);
          }
          const held = existing.roles.map((r) => r.id);
          if (held.length !== 1 || held[0] !== role.id) {
            expect(
              await api(superAdmin, "PUT", `/system/users/${existing.id}/roles`, { roleIds: [role.id], expectedRoleIds: held }),
              200,
              `giving ${user.email} the ${user.role} role`,
            );
          }
          continue;
        }
        check(reset.status === 403, `resetting ${user.email}: HTTP ${reset.status}`);
        user.email = user.email.replace("@", `+${run}@`);
      }
      const made = expect(
        await api(superAdmin, "POST", "/system/users", { name: user.name, email: user.email, password: user.password, roleIds: [role.id] }),
        201,
        `creating ${user.email}`,
      );
      accounts[kind] = made.id;
    }
    return Object.values(USERS).map((u) => `${u.email} (${u.role})`).join(", ");
  });

  await step("point the settings at the local frontend and issue a Content API key", async () => {
    const general = expect(await api(superAdmin, "GET", "/settings/general"), 200, "reading general settings");
    const integrations = expect(await api(superAdmin, "GET", "/settings/integrations"), 200, "reading integrations settings");
    check(!integrations.data.webhook_url, "a webhook URL is set: clear it first (its secret could not be put back)");
    const before = { frontend_url: general.data.frontend_url ?? null, preview_path: integrations.data.preview_path };
    cleanups.push(["put the settings back", async () => {
      await saveSettings(superAdmin, "integrations", { webhook_url: null, webhook_secret: null, preview_path: before.preview_path });
      await saveSettings(superAdmin, "general", { frontend_url: before.frontend_url });
    }]);
    await saveSettings(superAdmin, "general", { frontend_url: frontend });
    const saved = await api(superAdmin, "PUT", "/settings/integrations", {
      version: integrations.version,
      data: { webhook_url: `${frontend}/webhook`, webhook_secret: SECRET, preview_path: "/preview" },
    });
    check(saved.status === 200, `saving the webhook: HTTP ${saved.status} ${JSON.stringify(saved.body).slice(0, 300)} (start the server with RUSTCMS_WEBHOOK__ALLOW_INSECURE_LOCAL=true)`);
    const made = expect(await api(superAdmin, "POST", "/x/integrations/content-keys", { name: `E2E ${run}`, expires_in_days: 30 }), 201, "issuing a key");
    key = made.token;
    cleanups.push(["revoke the key", async () => {
      const r = await api(superAdmin, "DELETE", `/x/integrations/content-keys/${made.key.id}`);
      check(r.status === 204 || r.status === 404, `revoking the key: HTTP ${r.status}`);
    }]);
    return `${frontend}, key ${made.key.id}`;
  });

  let contributor;
  await step("sign in as the Contributor", async () => {
    contributor = await signIn(USERS.contributor.email, USERS.contributor.password);
  });

  await step("the Contributor writes a draft", async () => {
    await contributor.goto(`${admin}/r/posts/create`);
    await contributor.getByLabel(/^Title/).fill(TITLE);
    const created = contributor.waitForResponse((r) => r.request().method() === "POST" && /\/api\/resources\/posts\/records$/.test(new URL(r.url()).pathname));
    await contributor.getByRole("button", { name: /^Create$/ }).click();
    const response = await created;
    check(response.status() === 201 || response.status() === 200, `creating the post: HTTP ${response.status()}`);
    const body = await response.json();
    postId = String(body.data?.key ?? "");
    check(postId, `no id in ${JSON.stringify(body).slice(0, 200)}`);
    cleanups.push([`trash post ${postId}`, async () => {
      const r = await api(superAdmin, "DELETE", `/resources/posts/records/${postId}`);
      check(r.status === 204 || r.status === 200 || r.status === 404, `trashing the post: HTTP ${r.status}`);
    }]);
    return `post ${postId}`;
  });

  await step("the preview link from the preview slot opens, and the API answers it", async () => {
    await contributor.goto(`${admin}/r/posts/records/${postId}`);
    await contributor.getByRole("button", { name: /^Get preview link$/ }).click();
    const link = contributor.locator(".rblog-preview-link a");
    await link.waitFor({ timeout: 10000 });
    const href = await link.getAttribute("href");
    check(href?.startsWith(`${frontend}/preview?token=`), `unexpected link ${href}`);
    const opened = contributor.context().waitForEvent("page");
    await link.click();
    const tab = await opened;
    await tab.waitForLoadState();
    check((await tab.locator("h1").innerText()) === TITLE, "the preview page does not show the post");
    const last = previews.at(-1);
    check(last?.status === 200 && last.json?.preview?.status === "draft", `preview API: ${JSON.stringify(last).slice(0, 200)}`);
    await tab.close();
    return `${new URL(href).pathname}?token=… → /api/v1/preview 200, status draft`;
  });

  await step("the Contributor submits it and cannot publish", async () => {
    const row = await postRow(contributor);
    check((await row.getByRole("button", { name: /^Publish$/ }).count()) === 0, "a Publish button is shown to the Contributor");
    check((await row.getByRole("button", { name: /^Schedule/ }).count()) === 0, "a Schedule button is shown to the Contributor");
    // Nor for a selection (bulk actions such as Publish): rows are selectable only with a bulk
    // action the user may run.
    const select = row.getByRole("checkbox");
    if (await select.count()) {
      await select.check();
      await contributor.waitForTimeout(300);
      check((await contributor.getByRole("button", { name: /^Publish$/ }).count()) === 0, "Publish is offered to the Contributor for a selection");
      await select.uncheck();
    }
    await runAction(contributor, "submit", () => row.getByRole("button", { name: /^Submit for review$/ }).click());
    await until("the pending status", async () => (await (await postRow(contributor)).innerText()).includes("Pending review"), 15000);
    const refused = await api(contributor, "POST", "/resources/posts/actions/publish", { keys: [postId] });
    check(refused.status === 403 || refused.status === 404, `the server let the Contributor publish: HTTP ${refused.status}`);
    const menu = (await postRow(contributor)).getByRole("button", { name: /^Publish$/ });
    check((await menu.count()) === 0, "Publish shown after submitting");
    return `pending; publish refused with HTTP ${refused.status}`;
  });

  await step("the Editor publishes it", async () => {
    const editor = await signIn(USERS.editor.email, USERS.editor.password);
    const row = await postRow(editor);
    // Publish is a bulk action, and RustCMS 0.9.0 offers bulk actions only for a selection (not
    // in the row): select the row, then Publish in the selection bar.
    await row.getByRole("checkbox").check();
    await editor.getByRole("button", { name: /^Publish$/ }).first().click();
    const dialog = editor.getByRole("alertdialog").or(editor.getByRole("dialog"));
    await dialog.waitFor({ timeout: 5000 });
    await runAction(editor, "publish", () => dialog.getByRole("button", { name: /^(Publish|Confirm|Run)$/ }).last().click());
    await until("the published status", async () => (await (await postRow(editor)).innerText()).includes("Published"), 15000);
    await editor.context().close();
    return "published";
  });

  let slug;
  await step("the Content API returns it", async () => {
    const list = await fetch(`${base}/api/v1/content/posts`, { headers: { Authorization: `Bearer ${key}`, Accept: "application/json" } });
    check(list.status === 200, `GET /content/posts: HTTP ${list.status}`);
    const listed = (await list.json()).data?.find((p) => p.title === TITLE);
    check(listed, "the post is not in /content/posts");
    slug = listed.slug;
    const one = await fetch(`${base}/api/v1/content/posts/${encodeURIComponent(slug)}`, { headers: { Authorization: `Bearer ${key}` } });
    check(one.status === 200 && (await one.json()).title === TITLE, `GET /content/posts/${slug}: HTTP ${one.status}`);
    const anonymous = await fetch(`${base}/api/v1/content/posts/${encodeURIComponent(slug)}`);
    check(anonymous.status === 401, `without the key: HTTP ${anonymous.status}`);
    return `/content/posts and /content/posts/${slug} 200, 401 without the key`;
  });

  await step("the webhook receiver gets a signed post.published", async () => {
    const event = await until("the post.published webhook", () =>
      webhooks.find((w) => {
        try {
          const body = JSON.parse(w.body.toString("utf8"));
          return body.kind === "post.published" && body.subject?.id === postId;
        } catch {
          return false;
        }
      }),
    );
    const problem = verify(SECRET, event.headers, event.body);
    check(!problem, problem);
    check(verify(`${SECRET}x`, event.headers, event.body) === "bad signature", "a wrong secret also verifies");
    const body = JSON.parse(event.body.toString("utf8"));
    return `event ${body.id}, signature ok, tags ${body.tags.join(" ")}, paths ${JSON.stringify(body.paths)}; ${webhooks.length} webhook(s) received`;
  });
} catch {
  failed = true;
} finally {
  for (const [what, cleanup] of cleanups.reverse()) {
    try {
      await cleanup();
      console.log(`undo ${what}`);
    } catch (err) {
      failed = true;
      console.log(`CLEANUP FAILED ${what}: ${String(err?.message ?? err).split("\n")[0]}`);
    }
  }
  await browser.close();
  server.close();
}
console.log(failed ? "e2e smoke: FAILED" : "e2e smoke: passed");
process.exit(failed ? 1 : 0);
