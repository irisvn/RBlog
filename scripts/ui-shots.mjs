// Screenshots of every RBlog admin screen, for UI reviews (docs/ui-guidelines.md §10): each
// shot at each width, theme and language, signed in as a super admin of a running panel
// (`rblog serve`). Uses the installed Google Chrome through playwright-core (nothing is
// downloaded): run `npm ci` in resources/admin first. Modelled on RDockPanel's script.
//
//   RBLOG_SHOT_PASSWORD=… node scripts/ui-shots.mjs [out-dir]
//
// Environment (all optional but the password, given directly or in RBLOG_SHOT_PASSWORD_FILE):
//   RBLOG_SHOT_BASE      the admin URL (http://localhost:8000/admin)
//   RBLOG_SHOT_EMAIL     the user (shot@example.com): a super admin with no saved language
//                        (a saved one wins over RBLOG_SHOT_LOCALES)
//   RBLOG_SHOT_SHOTS     names to take, comma-separated (all)
//   RBLOG_SHOT_WIDTHS    1440,768,390
//   RBLOG_SHOT_THEMES    light,dark
//   RBLOG_SHOT_LOCALES   en,vi
//   RBLOG_SHOT_SCALE     the device pixel ratio (1)
//   RBLOG_SHOT_POST      the post whose page and edit form are shot (1), and its title
//   RBLOG_SHOT_POST_TITLE (Ba ngày ở Đà Nẵng): the list row whose Schedule dialog is shot
//   RBLOG_SHOT_RICH_POST the post whose rich-text editor is shot (RBLOG_SHOT_POST): it must hold
//                        an image, a table, a callout and a video embed
//   RBLOG_SHOT_PAGE      the page whose view (preview slot) and edit form are shot (1)
//   RBLOG_SHOT_AUTHOR    the author profile (its user id) whose edit form is shot (4)
//
// The other screens use the demo data (`rblog db:seed --class demo`). Set General → Public site
// URL first: without it the preview slot only says that it is missing.
//
// The Content API shots make a key named "Shot key" and revoke it again by its id at the end,
// even when a step fails (a failed revocation fails the run), so no key it made stays usable. Keep the shots out of the repository (the default is /tmp).
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";

const require = createRequire(new URL("../resources/admin/package.json", import.meta.url));
const { chromium } = require("playwright-core");

const env = (name, fallback) => process.env[name]?.trim() || fallback;
const list = (name, fallback) => env(name, fallback).split(",").map((s) => s.trim()).filter(Boolean);
const base = env("RBLOG_SHOT_BASE", "http://localhost:8000/admin");
const email = env("RBLOG_SHOT_EMAIL", "shot@example.com");
const passwordFile = process.env.RBLOG_SHOT_PASSWORD_FILE;
const password = process.env.RBLOG_SHOT_PASSWORD || (passwordFile && fs.readFileSync(passwordFile, "utf8").trim());
if (!password) {
  console.error("RBLOG_SHOT_PASSWORD (or RBLOG_SHOT_PASSWORD_FILE) is required");
  process.exit(2);
}
const out = process.argv[2] ?? "/tmp/rblog-ui-shots";
const widths = list("RBLOG_SHOT_WIDTHS", "1440,768,390").map(Number);
const themes = list("RBLOG_SHOT_THEMES", "light,dark");
const locales = list("RBLOG_SHOT_LOCALES", "en,vi");
const scale = Number(env("RBLOG_SHOT_SCALE", "1"));

const KEY_NAME = "Shot key";
const KEYS_PATH = "/integrations/content-keys";
const settle = (page, ms = 600) => page.waitForTimeout(ms);
const dialog = (page) => page.getByRole("dialog");

// A sequence of shots on one page: [name, what to do before the shot, full page?]. Each step
// starts where the previous one left off.
const SEQUENCES = [
  {
    path: "/p/content-api",
    steps: [
      ["content-api", async () => {}, true],
      ["content-api-new", async (page) => {
        await page.getByRole("button", { name: /^(New key|Tạo khoá)$/ }).first().click();
        await settle(page);
        await dialog(page).locator("input").first().fill(KEY_NAME);
        await dialog(page).locator("select").selectOption("90");
      }],
      ["content-api-invalid", async (page) => {
        await dialog(page).locator("input").first().fill("   ");
        await dialog(page).locator("button[type=submit]").click();
        await settle(page, 800);
      }],
      ["content-api-shown-once", async (page, state) => {
        await dialog(page).locator("input").first().fill(KEY_NAME);
        // The key's id, so that cleanup revokes exactly this key whatever fails afterwards.
        const made = page.waitForResponse((r) => r.request().method() === "POST" && r.url().endsWith(KEYS_PATH));
        await dialog(page).locator("button[type=submit]").click();
        const response = await made;
        if (response.status() !== 201) throw new Error(`creating the key: HTTP ${response.status()}`);
        state.keyId = (await response.json()).key.id;
        await dialog(page).locator(".content-api-secret").waitFor({ timeout: 8000 });
        await settle(page);
      }],
      ["content-api-keys", async (page) => {
        await dialog(page).getByRole("button", { name: /^(Close|Đóng)$/ }).last().click();
        await settle(page, 1000);
      }, true],
      ["content-api-revoke", async (page) => {
        const row = page.getByRole("row").filter({ hasText: KEY_NAME }).first();
        await row.getByRole("button").last().click();
        await settle(page, 300);
        await page.getByRole("menuitem", { name: /Revoke|Thu hồi/ }).click();
        await settle(page);
      }],
    ],
    // Revokes the key the shots made, by its id, through the page's own session (a 404: it was
    // revoked already). Throws when it could not: the key must not stay usable.
    cleanup: async (page, state) => {
      if (!state.keyId) return;
      const status = await page.evaluate(
        async ([admin, keys, id]) => {
          const csrf = await fetch(`${admin}/api/auth/csrf`, { credentials: "same-origin" }).then((r) => r.json());
          const response = await fetch(`${admin}/api/x${keys}/${encodeURIComponent(id)}`, {
            method: "DELETE",
            credentials: "same-origin",
            headers: { "X-CSRF-Token": csrf.csrfToken },
          });
          return response.status;
        },
        [new URL(base).pathname.replace(/\/$/, ""), KEYS_PATH, state.keyId],
      );
      if (status !== 204 && status !== 404) throw new Error(`revoking key ${state.keyId}: HTTP ${status}`);
    },
  },
  // Posts: the list and the Schedule dialog of the post RBLOG_SHOT_POST (1, titled
  // RBLOG_SHOT_POST_TITLE) from its row, then its page (preview slot, revisions) and its edit
  // form (the editor layout, SEO opened). Nothing is saved: the dialog is closed with Escape.
  {
    path: "/r/posts",
    steps: [
      ["posts-list", async () => {}, true],
      // The row's ⋯ menu: the transitions that apply to it, Preview, Unpublish last.
      ["posts-row-menu", async (page) => {
        const row = page.getByRole("row").filter({ hasText: env("RBLOG_SHOT_POST_TITLE", "Ba ngày ở Đà Nẵng") }).first();
        await row.getByRole("button", { name: /^(More actions for|Thao tác khác cho) / }).click();
        await settle(page);
      }],
      ["post-schedule", async (page) => {
        await page.keyboard.press("Escape");
        await settle(page, 300);
        const row = page.getByRole("row").filter({ hasText: env("RBLOG_SHOT_POST_TITLE", "Ba ngày ở Đà Nẵng") }).first();
        await openAction(page, row, /^(Schedule…|Hẹn giờ…)$/);
        await dialog(page).waitFor({ timeout: 5000 });
        await settle(page);
      }],
      // The row's Preview action: a new tab (closed here) and its notice with the link.
      ["post-preview-action", async (page) => {
        await page.keyboard.press("Escape");
        await settle(page, 300);
        const row = page.getByRole("row").filter({ hasText: env("RBLOG_SHOT_POST_TITLE", "Ba ngày ở Đà Nẵng") }).first();
        const popup = page.context().waitForEvent("page", { timeout: 5000 }).catch(() => null);
        await openAction(page, row, /^(Preview|Xem trước)$/);
        const tab = await popup;
        await tab?.close();
        await settle(page, 1000);
      }],
    ],
  },
  {
    path: `/r/posts/records/${env("RBLOG_SHOT_POST", "1")}`,
    steps: [
      ["post-view", async () => {}, true],
      ["post-preview", async (page) => {
        await getPreviewLink(page);
      }],
      // Its Revisions table: Restore this version is each row's labelled action.
      ["post-revisions", async (page) => {
        await page.getByRole("table").last().scrollIntoViewIfNeeded();
        await settle(page);
      }],
      // A revision of the post, from the last table of its page (Revisions).
      ["post-revision", async (page) => {
        await page.getByRole("table").last().getByRole("link", { name: /^(View|Xem)$/ }).first().click();
        await page.waitForURL(/\/r\/post_revisions\/records\//, { timeout: 8000 });
        await page.waitForLoadState("networkidle", { timeout: 8000 }).catch(() => {});
        await settle(page);
      }, true],
    ],
  },
  // The editor layout (RustCMS 0.10.2): the page as the window shows it (the sticky aside beside
  // the main column), then the folded SEO section opened and scrolled to.
  screen(`/r/posts/records/${env("RBLOG_SHOT_POST", "1")}`, "post-view-layout", false),
  {
    path: `/r/posts/records/${env("RBLOG_SHOT_POST", "1")}/edit`,
    steps: [
      ["post-edit-layout", async () => {}],
      ["post-edit-content", async () => {}, true],
      ["post-edit-seo-open", async (page) => {
        const seo = page.getByRole("button", { name: /^SEO$/ });
        await seo.click();
        await settle(page);
        await page.getByLabel(/^(Meta title|Tiêu đề SEO)/).scrollIntoViewIfNeeded();
        await settle(page, 300);
      }],
    ],
  },
  // The rich-text editor (RustCMS 0.10) on RBLOG_SHOT_RICH_POST, a post holding a captioned
  // image, a table, a callout and a video embed: the form, the embed card, the image details
  // dialog and the two media library pickers (body images, cover). Nothing is saved: every
  // dialog is closed with Escape.
  {
    path: `/r/posts/records/${env("RBLOG_SHOT_RICH_POST", env("RBLOG_SHOT_POST", "1"))}/edit`,
    steps: [
      ["post-edit-rich", async () => {}, true],
      ["post-embed-card", async (page) => {
        await richEditor(page).locator("figure[data-embed]").scrollIntoViewIfNeeded();
        await settle(page, 300);
      }],
      ["post-image-details", async (page) => {
        const image = richEditor(page).locator("figure[data-rich-image] img").first();
        await image.scrollIntoViewIfNeeded();
        await image.click();
        await page.getByRole("button", { name: /^(Image details|Chi tiết ảnh)$/ }).click();
        await dialog(page).waitFor({ timeout: 5000 });
        await settle(page);
      }],
      ["post-media-picker", async (page) => {
        await page.keyboard.press("Escape");
        await settle(page, 300);
        await page.getByRole("button", { name: /^(Insert image|Chèn ảnh)$/ }).click();
        await dialog(page).waitFor({ timeout: 5000 });
        await page.waitForLoadState("networkidle", { timeout: 8000 }).catch(() => {});
        await settle(page, 1000);
      }],
      ["post-cover-picker", async (page) => {
        await page.keyboard.press("Escape");
        await settle(page, 300);
        await page.getByRole("button", { name: /^(Media library|Thư viện tệp)$/ }).first().click();
        await dialog(page).waitFor({ timeout: 5000 });
        await page.waitForLoadState("networkidle", { timeout: 8000 }).catch(() => {});
        await settle(page, 1000);
      }],
    ],
  },
  // The other generated screens: one full-page shot each (nothing is saved). The preview slot
  // needs General → Public site URL (and Integrations → Preview path) to show its link.
  screen("/", "dashboard"),
  screen("/r/pages", "pages-list"),
  {
    path: `/r/pages/records/${env("RBLOG_SHOT_PAGE", "1")}`,
    steps: [
      ["page-view", async () => {}, true],
      ["page-preview", async (page) => {
        await getPreviewLink(page);
      }],
    ],
  },
  {
    path: `/r/pages/records/${env("RBLOG_SHOT_PAGE", "1")}/edit`,
    steps: [
      ["page-edit-layout", async () => {}],
      ["page-edit", async () => {}, true],
    ],
  },
  screen("/r/categories", "categories-list"),
  screen("/r/categories/records/1/edit", "category-edit"),
  screen("/r/tags", "tags-list"),
  screen("/r/tags/records/1/edit", "tag-edit"),
  screen("/r/menus", "menus-list"),
  screen("/r/menus/records/1", "menu-view"),
  screen("/r/menu_items/records/1/edit", "menu-item-edit"),
  screen("/r/authors", "authors-list"),
  screen(`/r/authors/records/${env("RBLOG_SHOT_AUTHOR", "4")}/edit`, "author-edit"),
  screen("/r/media", "media-library"),
  screen("/settings/general", "settings-general"),
  screen("/settings/reading", "settings-reading"),
  screen("/settings/seo", "settings-seo"),
  screen("/settings/integrations", "settings-integrations"),
  // The Version page and the dashboard (its notice of a newer release, when the last check found
  // one: run the app with `[releases] url` on a local list of releases to show it).
  screen("/p/version", "version"),
  screen("/", "dashboard"),
];

/** A sequence of one shot `name` of `path`, of the full page unless `full` is false. */
function screen(path, name, full = true) {
  return { path, steps: [[name, async () => {}, full]] };
}

/** The post form's rich-text editor (its body). */
function richEditor(page) {
  return page.locator("[contenteditable=true]").first();
}

/** Asks the preview slot for a link (a preview token, valid one hour) and waits for it. */
async function getPreviewLink(page) {
  const button = page.getByRole("button", { name: /^(Get preview link|Lấy link xem trước)$/ });
  await button.scrollIntoViewIfNeeded();
  await button.click();
  await settle(page, 1000);
}

/** Clicks the action `name` of `row`, opening its ⋯ menu first when it is not a button. */
async function openAction(page, row, name) {
  const direct = row.getByRole("button", { name });
  if (await direct.count()) {
    await direct.first().click();
    return;
  }
  await row.getByRole("button", { name: /^(More actions for|Thao tác khác cho) / }).click();
  await settle(page, 300);
  await page.getByRole("menuitem", { name }).click();
}
const wanted = list("RBLOG_SHOT_SHOTS", SEQUENCES.flatMap((s) => s.steps.map(([name]) => name)).join(","));

fs.mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: "chrome" });
let failed = 0;
const shots = [];
for (const locale of locales) {
  for (const theme of themes) {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: scale, colorScheme: theme, locale });
    // The shell's own choices (theme, language), before any page loads.
    await context.addInitScript(([scheme, lang]) => {
      try {
        localStorage.setItem("rustcms.colorScheme", scheme);
        localStorage.setItem("rustcms-admin-locale", lang);
      } catch {}
    }, [theme, locale]);
    const page = await context.newPage();
    await page.goto(`${base}/login`);
    await page.locator("input[type=email]").fill(email);
    await page.locator("input[type=password]").fill(password);
    await page.locator("button[type=submit]").click();
    await page.waitForURL((url) => !url.pathname.endsWith("/login"), { timeout: 20000 });
    for (const width of widths) {
      await page.setViewportSize({ width, height: 900 });
      for (const sequence of SEQUENCES) {
        // A sequence none of whose shots is wanted does not run (its steps may write).
        if (!sequence.steps.some(([name]) => wanted.includes(name))) continue;
        // What the steps made (`keyId`), for the cleanup.
        const state = {};
        try {
          const response = await page.goto(base + sequence.path);
          if (response && response.status() >= 400) throw new Error(`HTTP ${response.status()}`);
          await page.waitForLoadState("networkidle", { timeout: 8000 }).catch(() => {});
          await settle(page, 1200);
          const lang = await page.evaluate(() => document.documentElement.lang);
          if (lang && !lang.startsWith(locale)) throw new Error(`shown in ${lang}, not ${locale} (the user's saved language wins)`);
          for (const [name, act, full] of sequence.steps) {
            await act(page, state);
            if (!wanted.includes(name)) continue;
            const file = path.join(out, `${name}-${width}-${theme}-${locale}.png`);
            // A page wider than the window scrolls sideways: a layout bug.
            const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
            if (overflow > 0) console.log("OVERFLOW", path.basename(file), `${overflow}px`);
            await page.screenshot({ path: file, fullPage: Boolean(full) });
            shots.push(file);
            console.log("shot", path.basename(file));
          }
        } catch (err) {
          failed += 1;
          console.log("FAILED", sequence.path, width, theme, locale, String(err.message).split("\n")[0]);
        } finally {
          // Leaves any dialog first, so that a broken step cannot submit a form here.
          await page.keyboard.press("Escape").catch(() => {});
          try {
            await sequence.cleanup?.(page, state);
          } catch (err) {
            failed += 1;
            console.log("CLEANUP FAILED", sequence.path, width, theme, locale, String(err.message).split("\n")[0]);
          }
        }
      }
    }
    await context.close();
  }
}
await browser.close();
console.log(`${shots.length} shots in ${out}`);
process.exit(failed ? 1 : 0);
