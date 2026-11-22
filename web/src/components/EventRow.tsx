import { useTranslation } from "react-i18next";
import { formatDate, timeAgo } from "../lib/format";
import type { Event } from "../lib/types";
import { Icon, type IconName } from "./Icon";

const STYLE: Record<string, { icon: IconName; cls: string }> = {
  "client.connected": { icon: "plug", cls: "bg-success/15 text-success" },
  "client.disconnected": { icon: "plug", cls: "bg-base-300 text-base-content/60" },
  "client.rejected": { icon: "alert", cls: "bg-warning/15 text-warning" },
  "service.failed": { icon: "alert", cls: "bg-error/15 text-error" },
  "auth.failed": { icon: "lock", cls: "bg-warning/15 text-warning" },
  "auth.login": { icon: "user", cls: "bg-info/15 text-info" },
};

export function EventRow({ event, compact }: { event: Event; compact?: boolean }) {
  const { t } = useTranslation();
  const style = STYLE[event.kind] ?? {
    icon: event.kind.startsWith("service") ? "services" : event.kind.startsWith("user") ? "user" : "clients",
    cls: "bg-primary/10 text-primary",
  };
  return (
    <li className="flex items-start gap-3">
      <span className={`mt-0.5 grid size-8 shrink-0 place-items-center rounded-full ${style.cls}`}>
        <Icon name={style.icon} className="size-4" />
      </span>
      <div className="min-w-0 flex-1">
        <p className="text-sm">
          {t(`events.${event.kind}`, { subject: event.subject, defaultValue: `${event.kind}: ${event.subject}` })}
        </p>
        <p className="text-base-content/50 truncate text-xs" title={formatDate(event.at)}>
          {timeAgo(event.at)}
          {event.actor && ` · ${t("activity.by", { actor: event.actor })}`}
          {!compact && event.detail && ` · ${event.detail}`}
        </p>
      </div>
    </li>
  );
}
