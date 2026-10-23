// The pieces RBlog's React pages are built from (docs/ui-guidelines.md §7), on the panel's UI kit
// (`ui` 2) and theme tokens, so they follow light and dark mode. Adapted from RDockPanel's
// `rdockpanel/lib/ui.tsx`; add a component here when a page first needs it.
import { useEffect, useId, useRef, type ReactNode } from "react";
import type { PageProps } from "@rustcms/admin-sdk";
import { Button, cn, Dialog, DropdownMenu, DropdownMenuItem, DropdownMenuSeparator, PageHeader, Skeleton, Tooltip } from "@rustcms/admin-sdk/ui";
import { Icon, type IconName } from "./icons";
import type { Tone } from "./status";
import "./ui.css";

type T = (text: string) => string;

/** `t(text)` with its `{name}` placeholders filled. */
export function fill(text: string, values: Record<string, string | number>): string {
  return text.replace(/\{(\w+)\}/g, (all, key: string) => (key in values ? String(values[key]) : all));
}

// ---------------------------------------------------------------- Layout

/** A page: its header (eyebrow, title, description, actions), then its sections 24px apart. The
 * shell's own heading is hidden (it stays for screen readers): this one replaces it. */
export function Page(
  props: Pick<PageProps<unknown>, "title"> & {
    eyebrow?: ReactNode;
    heading: ReactNode;
    description?: ReactNode;
    actions?: ReactNode;
    children?: ReactNode;
  },
) {
  const { title } = props;
  useEffect(() => {
    title(null);
  }, [title]);
  return (
    <div className="rblog-page">
      <PageHeader eyebrow={props.eyebrow} title={props.heading} description={props.description} actions={props.actions} />
      {props.children}
    </div>
  );
}

/** The vertical rhythm of a page's sections (24px). */
export function Stack(props: { children?: ReactNode }) {
  return <div className="rblog-stack">{props.children}</div>;
}

/** A section: a card with a header (title, description, actions) and a body. `flush`: the body
 * reaches the card's edges (a table). */
export function Section(props: { title: ReactNode; description?: ReactNode; actions?: ReactNode; children?: ReactNode; flush?: boolean }) {
  const id = useId();
  return (
    <section className="rblog-section shadow-xs" aria-labelledby={id}>
      <header className="rblog-section-head">
        <div className="rblog-section-text">
          <h2 id={id} className="rblog-section-title">
            {props.title}
          </h2>
          {props.description && <p className="rblog-section-description">{props.description}</p>}
        </div>
        {props.actions && <div className="rblog-actions">{props.actions}</div>}
      </header>
      {props.children != null && <div className={props.flush ? "rblog-section-flush" : "rblog-section-body"}>{props.children}</div>}
    </section>
  );
}

/** A `Section` whose body is a table that scrolls sideways inside the card on a narrow screen, with
 * an optional toolbar, its empty state (`empty`, drawn instead of the table) and a footer (`Pager`). */
export function TableCard(props: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  toolbar?: ReactNode;
  empty?: ReactNode;
  footer?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <Section flush title={props.title} description={props.description} actions={props.actions}>
      {props.toolbar && <div className="rblog-toolbar">{props.toolbar}</div>}
      {props.empty ?? <div className="rblog-table-scroll">{props.children}</div>}
      {!props.empty && props.footer}
    </Section>
  );
}

/** A table's rows while they load: the same columns, so nothing jumps. */
export function TableSkeleton(props: { columns: number; rows?: number }) {
  const rows = Array.from({ length: props.rows ?? 3 }, (_, i) => i);
  const columns = Array.from({ length: props.columns }, (_, i) => i);
  return (
    <div className="rblog-skeleton-rows" aria-hidden>
      {rows.map((row) => (
        <div key={row} className="rblog-skeleton-row">
          {columns.map((column) => (
            <Skeleton key={column} className="rblog-skeleton-cell" />
          ))}
        </div>
      ))}
    </div>
  );
}

/** `1–25 of 132` with Previous and Next (25 rows a page). */
export function Pager(props: { page: number; perPage: number; total: number; onPage: (page: number) => void; t: T }) {
  const last = Math.max(1, Math.ceil(props.total / props.perPage));
  const from = props.total === 0 ? 0 : (props.page - 1) * props.perPage + 1;
  const to = Math.min(props.total, props.page * props.perPage);
  return (
    <div className="rblog-table-foot">
      <span className="rblog-num">{fill(props.t("{from}–{to} of {total}"), { from, to, total: props.total })}</span>
      {last > 1 && (
        <span className="rblog-actions">
          <Button size="sm" variant="outline" disabled={props.page <= 1} onClick={() => props.onPage(props.page - 1)}>
            {props.t("Previous")}
          </Button>
          <Button size="sm" variant="outline" disabled={props.page >= last} onClick={() => props.onPage(props.page + 1)}>
            {props.t("Next")}
          </Button>
        </span>
      )}
    </div>
  );
}

/** Nothing to show yet: a line, an optional hint and an optional action, centered. */
export function EmptyState(props: { title: string; hint?: string; action?: ReactNode }) {
  return (
    <div className="rblog-empty">
      <p className="rblog-empty-title">{props.title}</p>
      {props.hint && <p className="rblog-empty-hint">{props.hint}</p>}
      {props.action && <div className="rblog-empty-action">{props.action}</div>}
    </div>
  );
}

/** Label-over-value facts of one thing; `mono` per item for values that are code. */
export function Facts(props: { items: Array<{ label: string; value: ReactNode; mono?: boolean }> }) {
  return (
    <dl className="rblog-facts">
      {props.items.map((item) => (
        <div key={item.label} className="rblog-fact">
          <dt>{item.label}</dt>
          <dd className={item.mono ? "rblog-mono" : undefined}>{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}

// ---------------------------------------------------------------- Buttons

/** An icon-only button (32px, ghost) with its words in a tooltip and for screen readers. */
export function IconButton(props: { icon: IconName; label: string; onClick?: () => void; disabled?: boolean }) {
  return (
    <Tooltip content={props.label}>
      <Button type="button" variant="ghost" size="icon" className="rblog-icon-sm" aria-label={props.label} disabled={props.disabled} onClick={props.onClick}>
        <Icon name={props.icon} />
      </Button>
    </Tooltip>
  );
}

export type MenuItem = { label: string; onSelect: () => void; destructive?: boolean; disabled?: boolean };

/** A row's actions: up to two ghost buttons (`children`), then a `⋯` menu whose destructive items
 * come last, after a separator. `id` names the row, to give focus back to its menu. */
export function RowActions(props: { id: string; label: string; children?: ReactNode; menu?: MenuItem[] }) {
  const items = props.menu ?? [];
  const plain = items.filter((item) => !item.destructive);
  const destructive = items.filter((item) => item.destructive);
  return (
    <div className="rblog-row-actions" data-row-actions={props.id}>
      {props.children}
      {items.some((item) => !item.disabled) && (
        <DropdownMenu variant="ghost" size="icon" align="end" className="rblog-icon-sm" aria-label={props.label} trigger={<Icon name="more" />}>
          {plain.map((item) => (
            <DropdownMenuItem key={item.label} onSelect={item.onSelect} disabled={item.disabled}>
              {item.label}
            </DropdownMenuItem>
          ))}
          {plain.length > 0 && destructive.length > 0 && <DropdownMenuSeparator />}
          {destructive.map((item) => (
            <DropdownMenuItem key={item.label} onSelect={item.onSelect} disabled={item.disabled} destructive>
              {item.label}
            </DropdownMenuItem>
          ))}
        </DropdownMenu>
      )}
    </div>
  );
}

/** Focuses `target()` once a closing dialog is gone: the kit's `Dialog` is a native `<dialog>`,
 * which keeps focus inside while open and gives it back to whatever had it before (often a menu
 * item that no longer exists) when it closes. Call it from the close handler. */
export function focusAfterDialog(target: () => HTMLElement | null | undefined) {
  requestAnimationFrame(() => requestAnimationFrame(() => target()?.focus()));
}

/** Focus back on the menu button of row `id` once its dialog closed, else on `fallback`. */
export function focusRow(id: string, fallback: () => HTMLElement | null | undefined) {
  focusAfterDialog(() => document.querySelector<HTMLElement>(`[data-row-actions="${CSS.escape(id)}"] button`) ?? fallback());
}

// ---------------------------------------------------------------- Status

/** A status: a dot in its tone beside its word, in the text color (never color alone). */
export function StatusBadge(props: { tone: Tone; label: string; title?: string }) {
  return (
    <span className="rblog-badge" title={props.title}>
      <span className={cn("rblog-dot", `rblog-dot-${props.tone}`)} aria-hidden />
      {props.label}
    </span>
  );
}

// ---------------------------------------------------------------- Forms

/** A label above its control, the help below it, then the field's errors. `children` gets the ids
 * to put on the control (`id`, `aria-describedby`, `aria-invalid`). */
export function Field(props: {
  label: string;
  help?: string;
  error?: string[];
  children: (control: { id: string; "aria-describedby"?: string; "aria-invalid"?: boolean }) => ReactNode;
}) {
  const id = useId();
  const help = props.help ? `${id}-help` : undefined;
  const error = props.error?.length ? `${id}-error` : undefined;
  const describedBy = [help, error].filter(Boolean).join(" ") || undefined;
  return (
    <div className="rblog-field">
      <label htmlFor={id} className="rblog-field-label">
        {props.label}
      </label>
      {props.children({ id, "aria-describedby": describedBy, "aria-invalid": error ? true : undefined })}
      {props.help && (
        <p id={help} className="rblog-field-help">
          {props.help}
        </p>
      )}
      {error && (
        <p id={error} className="rblog-field-error">
          {props.error!.join(" ")}
        </p>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- Feedback

/** The confirmation of a destructive or disruptive action (never `window.confirm`), 440px wide.
 * Focus starts on Cancel; while `busy`, the action says what is happening and nothing closes it. */
export function ConfirmDialog(props: {
  open: boolean;
  title: string;
  body?: ReactNode;
  /** The confirm button's words ("Revoke key"). */
  action: string;
  /** Its words while the action runs ("Revoking…"). */
  busyAction: string;
  busy?: boolean;
  destructive?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  t: T;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (props.open) cancel.current?.focus();
  }, [props.open]);
  return (
    <Dialog open={props.open} onClose={props.onCancel} title={props.title} className="rblog-dialog-compact" closeDisabled={props.busy}>
      <form
        className="rblog-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (!props.busy) props.onConfirm();
        }}
      >
        {props.body && <div className="rblog-dialog-body">{props.body}</div>}
        <div className="rblog-dialog-foot">
          <Button ref={cancel} type="button" variant="ghost" disabled={props.busy} onClick={props.onCancel}>
            {props.t("Cancel")}
          </Button>
          <Button type="submit" variant={props.destructive ? "destructive" : "default"} disabled={props.busy}>
            {props.busy ? props.busyAction : props.action}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
