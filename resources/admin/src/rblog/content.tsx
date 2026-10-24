// Posts and pages: the status cell of their lists (`rblog/content-status`, docs/ui-guidelines.md
// §4: a dot and a word) and the preview slot of their record pages (`rblog/content-preview`),
// which asks `routes::admin` (`/previews/{posts|pages}/{id}`) for a link to the public site's
// preview route. The server checks who may preview the record.
import { useState } from "react";
import { registerCell, registerSlot, useAdminMutation, type CellProps, type SlotProps } from "@rustcms/admin-sdk";
import { ApiError } from "@rustcms/admin-sdk/host";
import { Alert, Button } from "@rustcms/admin-sdk/ui";
import { dateTime } from "./lib/format";
import "./lib/i18n";
import { Icon } from "./lib/icons";
import { contentStatus } from "./lib/status";
import { fill, IconButton, StatusBadge } from "./lib/ui";
import "./content.css";

function StatusCell(props: CellProps<unknown, unknown, unknown>) {
  const [tone, word] = contentStatus(props.value);
  if (!word) return null;
  return <StatusBadge tone={tone} label={props.t(word)} />;
}

type Link = { url: string | null; expires_at?: string };
type PreviewData = { route: string };

/** The words of a refused or failed request; `null` when the session changed. */
function message(error: unknown, t: (s: string) => string): string | null {
  if (error instanceof ApiError) {
    if (error.code === "session_changed") return null;
    if (error.status === 404 || error.status === 403) return t("This record can no longer be previewed.");
  }
  return t("Something went wrong. Try again.");
}

function PreviewSlot(props: SlotProps<PreviewData, unknown>) {
  const t = props.t;
  const [link, setLink] = useState<Link | null>(null);
  const make = useAdminMutation<Link>(props, props.data.route, "post");
  const ask = () => {
    make.mutate(undefined, {
      onSuccess: (made) => setLink(made),
      onError: (error) => {
        const text = message(error, t);
        if (text) props.toast.error(text);
      },
    });
  };
  const copy = async (url: string) => {
    try {
      await navigator.clipboard.writeText(url);
      props.toast.success(t("Link copied"));
    } catch {
      props.toast.error(t("Could not copy. Select the link and copy it."));
    }
  };
  return (
    <div className="rblog-preview">
      <p className="rblog-preview-text">
        {t("See it on the public site as readers will, before it goes live. A link works for one hour.")}
      </p>
      {link?.url === null && (
        <Alert className="rblog-alert rblog-alert-info">
          <Icon name="info" />
          <p>{t("Set the public site URL in Settings → General to preview.")}</p>
        </Alert>
      )}
      {link?.url && (
        <div className="rblog-preview-link">
          <a className="rblog-link rblog-ellipsis rblog-mono" href={link.url} target="_blank" rel="noreferrer noopener" title={link.url}>
            {link.url}
          </a>
          <IconButton icon="copy" label={t("Copy link")} onClick={() => void copy(link.url ?? "")} />
          {link.expires_at && (
            <p className="rblog-preview-expiry">{fill(t("Works until {time}"), { time: dateTime(link.expires_at, props.locale) })}</p>
          )}
        </div>
      )}
      <div className="rblog-actions">
        <Button type="button" variant="outline" size="sm" disabled={make.isPending} onClick={ask}>
          <Icon name="externalLink" />
          {make.isPending ? t("Making link…") : link?.url ? t("New preview link") : t("Get preview link")}
        </Button>
      </div>
    </div>
  );
}

registerCell("rblog/content-status", StatusCell);
registerSlot("rblog/content-preview", PreviewSlot);
