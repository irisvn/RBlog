// Integrations → Content API (`/admin/p/content-api`, `ContentApiPage`): the keys the public site
// reads published content with (docs/api.md). Listed, made (the secret shown once) and revoked
// through `routes::admin` (`/integrations/content-keys`); the server checks every request.
import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { adminKey, fieldErrors, registerPage, useAdminQuery, type PageProps } from "@rustcms/admin-sdk";
import { ApiError } from "@rustcms/admin-sdk/host";
import {
  Alert,
  Button,
  CodeBlock,
  Dialog,
  ErrorState,
  Input,
  NativeSelect,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@rustcms/admin-sdk/ui";
import { date, dateTime, relative } from "./lib/format";
import "./lib/i18n";
import { Icon } from "./lib/icons";
import { keyStatus } from "./lib/status";
import {
  ConfirmDialog,
  EmptyState,
  Facts,
  Field,
  fill,
  focusAfterDialog,
  focusRow,
  Page,
  Pager,
  RowActions,
  Section,
  Stack,
  StatusBadge,
  TableCard,
  TableSkeleton,
} from "./lib/ui";
import "./content-api.css";

const PER_PAGE = 25;
const MANAGE = "integrations.manage";
const DOCS = "https://github.com/irisvn/RBlog/blob/main/docs/api.md";

type Key = { id: string; name: string; created_at: string; last_used_at: string | null; expires_at: string | null };
type Keys = { data: Key[]; meta: { page: number; perPage: number; total: number; lastPage: number }; base_url: string };
type Created = { token: string; key: Key };

/** The words of an error the server or the network gave; `null` when the session changed (the
 * page is mounted again for the new one: nothing to say). */
function message(error: unknown, t: (s: string) => string): string | null {
  if (error instanceof ApiError) {
    if (error.code === "session_changed") return null;
    if (error.maybeApplied) return t("The answer was lost: the change may have been made. Check the list.");
    if (error.message) return error.message;
  }
  return t("Something went wrong. Try again.");
}

/** Shows `error` in a toast, unless the session changed. */
function toastError(props: PageProps<unknown>, error: unknown) {
  const text = message(error, props.t);
  if (text) props.toast.error(text);
}

function ContentApiPage(props: PageProps<unknown>) {
  const t = props.t;
  const [page, setPage] = useState(1);
  const keys = useAdminQuery<Keys>(props, "/integrations/content-keys", { query: { page, per_page: PER_PAGE } });
  // After a key is made or revoked every page of the list is stale, not only the one shown: drop
  // them all (the list's key without its query is their common prefix).
  const queries = useQueryClient();
  const reloadKeys = () => queries.invalidateQueries({ queryKey: adminKey(props, "/integrations/content-keys").slice(0, -1) });
  const canManage = props.can(MANAGE);
  const [creating, setCreating] = useState(false);
  const [revoking, setRevoking] = useState<Key | null>(null);
  const [busy, setBusy] = useState(false);
  const newKey = useRef<HTMLButtonElement>(null);

  // A page past the last one (its last key revoked): back to the last page.
  const lastPage = keys.data?.meta.lastPage;
  useEffect(() => {
    if (lastPage && page > lastPage) setPage(lastPage);
  }, [lastPage, page]);

  const openCreate = () => setCreating(true);
  const newButton = canManage ? (
    <Button ref={newKey} onClick={openCreate}>
      {t("New key")}
    </Button>
  ) : null;

  const revoke = async () => {
    if (!revoking) return;
    setBusy(true);
    try {
      await props.api.delete(`/integrations/content-keys/${encodeURIComponent(revoking.id)}`);
      props.toast.success(t("Key revoked"));
      setRevoking(null);
      focusAfterDialog(() => newKey.current);
      void reloadKeys();
    } catch (error) {
      toastError(props, error);
    } finally {
      setBusy(false);
    }
  };

  const data = keys.data;
  let table;
  if (keys.error) {
    table = (
      <Section title={t("Keys")}>
        <ErrorState error={keys.error} />
      </Section>
    );
  } else {
    table = (
      <TableCard
        title={t("Keys")}
        description={t("Each key reads published content only. Revoke a key you no longer use.")}
        empty={
          !data ? (
            <TableSkeleton columns={4} />
          ) : data.meta.total === 0 ? (
            <EmptyState
              title={t("No keys yet")}
              hint={t("Create a key for each site that reads your content.")}
              action={
                canManage && (
                  <Button variant="outline" size="sm" onClick={openCreate}>
                    {t("New key")}
                  </Button>
                )
              }
            />
          ) : undefined
        }
        footer={data && <Pager page={data.meta.page} perPage={PER_PAGE} total={data.meta.total} onPage={setPage} t={t} />}
      >
        {data && (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t("Name")}</TableHead>
                <TableHead>{t("Status")}</TableHead>
                <TableHead className="rblog-hide-md">{t("Created")}</TableHead>
                <TableHead className="rblog-hide-md">{t("Last used")}</TableHead>
                <TableHead className="rblog-hide-sm">{t("Expires")}</TableHead>
                {canManage && (
                  <TableHead className="rblog-actions-cell">
                    <span className="rblog-sr">{t("Actions")}</span>
                  </TableHead>
                )}
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.data.map((key) => {
                const [tone, word] = keyStatus(key.expires_at);
                return (
                  <TableRow key={key.id}>
                    <TableCell className="rblog-name-cell">
                      <span className="rblog-ellipsis" title={key.name}>
                        {key.name}
                      </span>
                    </TableCell>
                    <TableCell>
                      {/* On a phone the Expires column is hidden: the badge's tooltip keeps the date. */}
                      <StatusBadge
                        tone={tone}
                        label={t(word)}
                        title={key.expires_at ? fill(t("Expires {date}"), { date: dateTime(key.expires_at, props.locale) }) : t("Does not expire")}
                      />
                    </TableCell>
                    <TableCell className="rblog-hide-md rblog-nowrap" title={dateTime(key.created_at, props.locale)}>
                      {date(key.created_at, props.locale)}
                    </TableCell>
                    <TableCell className="rblog-hide-md rblog-nowrap" title={dateTime(key.last_used_at, props.locale)}>
                      {key.last_used_at ? relative(key.last_used_at, props.locale) : <span className="rblog-muted">{t("Not used yet")}</span>}
                    </TableCell>
                    <TableCell className="rblog-hide-sm rblog-nowrap" title={dateTime(key.expires_at, props.locale)}>
                      {key.expires_at ? date(key.expires_at, props.locale) : <span className="rblog-muted">{t("Never")}</span>}
                    </TableCell>
                    {canManage && (
                      <TableCell className="rblog-actions-cell">
                        <RowActions
                          id={key.id}
                          label={fill(t("Actions for {name}"), { name: key.name })}
                          menu={[{ label: t("Revoke…"), destructive: true, onSelect: () => setRevoking(key) }]}
                        />
                      </TableCell>
                    )}
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </TableCard>
    );
  }

  return (
    <Page
      title={props.title}
      eyebrow={t("Integrations")}
      heading={t("Content API")}
      description={t("Keys that let your public site read published posts, pages, menus and settings.")}
      actions={newButton}
    >
      <Stack>
        {table}
        <Usage {...props} baseUrl={data?.base_url} />
      </Stack>
      {creating && (
        <CreateKey
          {...props}
          onCreated={() => {
            setPage(1);
            void reloadKeys();
          }}
          onClose={() => {
            setCreating(false);
            focusAfterDialog(() => newKey.current);
          }}
        />
      )}
      <ConfirmDialog
        open={revoking != null}
        title={fill(t("Revoke the key {name}?"), { name: revoking?.name ?? "" })}
        body={<p className="rblog-dialog-body">{t("Sites that use it can no longer read your content. This cannot be undone.")}</p>}
        action={t("Revoke key")}
        busyAction={t("Revoking…")}
        busy={busy}
        destructive
        onConfirm={() => void revoke()}
        onCancel={() => {
          const id = revoking?.id;
          setRevoking(null);
          if (id) focusRow(id, () => newKey.current);
        }}
        t={t}
      />
    </Page>
  );
}

/** How the public site uses a key: the base URL, the header, and where the key must stay. */
function Usage(props: PageProps<unknown> & { baseUrl?: string }) {
  const t = props.t;
  const base = props.baseUrl ?? "";
  return (
    <Section title={t("Using a key")} description={t("Your public site sends the key with every request to the Content API.")}>
      <Facts
        items={[
          { label: t("Base URL"), value: base, mono: true },
          { label: t("Header"), value: "Authorization: Bearer rcms_…", mono: true },
        ]}
      />
      {base && <CodeBlock className="content-api-example" label={t("Example")} code={`curl -H "Authorization: Bearer rcms_…" ${base}/posts`} />}
      <Alert className="rblog-alert rblog-alert-warning">
        <Icon name="warning" />
        <p>{t("Keep the key on the server (SSR): never send it to browsers or put it in client code.")}</p>
      </Alert>
      <p className="rblog-small rblog-muted">
        <a className="rblog-link" href={DOCS} target="_blank" rel="noreferrer">
          {t("Content API documentation")}
          <Icon name="externalLink" size={12} />
        </a>
      </p>
    </Section>
  );
}

const EXPIRIES = [
  { value: "", label: "No expiry" },
  { value: "30", label: "30 days" },
  { value: "90", label: "90 days" },
  { value: "365", label: "365 days" },
];

/** New key (560px): name and expiry, then the key itself, shown once. */
function CreateKey(props: PageProps<unknown> & { onCreated: () => void; onClose: () => void }) {
  const t = props.t;
  const [name, setName] = useState("");
  const [expiry, setExpiry] = useState("");
  const [busy, setBusy] = useState(false);
  const [errors, setErrors] = useState<Record<string, string[]>>({});
  const [created, setCreated] = useState<Created | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const expiryInput = useRef<HTMLSelectElement>(null);

  useEffect(() => {
    nameInput.current?.focus();
  }, []);

  const submit = async () => {
    setBusy(true);
    setErrors({});
    try {
      const body = { name: name.trim(), expires_in_days: expiry ? Number(expiry) : null };
      const out = await props.api.post<Created>("/integrations/content-keys", body);
      setCreated(out);
      props.toast.success(t("Key created"));
      props.onCreated();
    } catch (error) {
      const fields = fieldErrors(error);
      if (Object.keys(fields).length > 0) {
        setErrors(fields);
        // The first field in error takes the focus, its message read with it.
        requestAnimationFrame(() => (fields.name ? nameInput.current : expiryInput.current)?.focus());
      } else {
        toastError(props, error);
      }
    } finally {
      setBusy(false);
    }
  };

  if (created) {
    return <ShownOnce {...props} created={created} />;
  }
  return (
    <Dialog open onClose={props.onClose} title={t("New key")} className="rblog-dialog-form" closeDisabled={busy}>
      <form
        className="rblog-form"
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          if (!busy) void submit();
        }}
      >
        <Field label={t("Name")} help={t("Where the key is used, for example Production site. At most 80 characters.")} error={errors.name}>
          {(control) => <Input {...control} ref={nameInput} value={name} maxLength={80} autoComplete="off" onChange={(e) => setName(e.target.value)} />}
        </Field>
        <Field label={t("Expires")} help={t("An expired key stops working. You can revoke a key at any time.")} error={errors.expires_in_days}>
          {(control) => (
            <NativeSelect {...control} ref={expiryInput} value={expiry} onChange={(e) => setExpiry(e.target.value)}>
              {EXPIRIES.map((option) => (
                <option key={option.value} value={option.value}>
                  {t(option.label)}
                </option>
              ))}
            </NativeSelect>
          )}
        </Field>
        <div className="rblog-dialog-foot">
          <Button type="button" variant="ghost" disabled={busy} onClick={props.onClose}>
            {t("Cancel")}
          </Button>
          <Button type="submit" disabled={busy}>
            {busy ? t("Creating…") : t("Create key")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

/** The new key, once: copy it now, it is never shown again. */
function ShownOnce(props: PageProps<unknown> & { created: Created; onClose: () => void }) {
  const t = props.t;
  const [copied, setCopied] = useState(false);
  const copy = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    copy.current?.focus();
  }, []);
  const copyKey = async () => {
    try {
      await navigator.clipboard.writeText(props.created.token);
      setCopied(true);
      props.toast.success(t("Key copied"));
    } catch {
      props.toast.error(t("Could not copy. Select the key and copy it."));
    }
  };
  return (
    <Dialog open onClose={props.onClose} title={t("Copy your new key")} className="rblog-dialog-form">
      <div className="rblog-form">
        <Alert className="rblog-alert rblog-alert-warning">
          <Icon name="warning" />
          <p>{t("Copy it now: it is not shown again.")}</p>
        </Alert>
        <p className="content-api-secret rblog-mono" aria-label={t("Content API key")}>
          {props.created.token}
        </p>
        <div className="rblog-dialog-foot">
          <Button type="button" variant="ghost" onClick={props.onClose}>
            {t("Close")}
          </Button>
          <Button ref={copy} type="button" onClick={() => void copyKey()}>
            <Icon name={copied ? "check" : "copy"} />
            {copied ? t("Copied") : t("Copy key")}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

registerPage("rblog/content-api", ContentApiPage);
