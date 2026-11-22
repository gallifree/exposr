import { motion } from "motion/react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ServiceStatus } from "../lib/types";
import { Icon, type IconName } from "./Icon";

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: string; actions?: ReactNode }) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{title}</h1>
        {subtitle && <p className="text-base-content/60 mt-1 text-sm">{subtitle}</p>}
      </div>
      {actions && <div className="flex gap-2">{actions}</div>}
    </div>
  );
}

export function ErrorAlert({ code }: { code: string | null }) {
  const { t } = useTranslation();
  if (!code) return null;
  return (
    <div role="alert" className="alert alert-error alert-soft my-3">
      <Icon name="alert" />
      <span>{t(`errors.${code}`, { defaultValue: t("errors.INTERNAL") })}</span>
    </div>
  );
}

export function Loading() {
  return (
    <div className="flex justify-center py-16">
      <span className="loading loading-dots loading-lg text-primary" />
    </div>
  );
}

export function Empty({ icon, title, hint, action }: { icon: IconName; title: string; hint?: string; action?: ReactNode }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      className="border-base-300 flex flex-col items-center gap-3 rounded-box border border-dashed px-6 py-14 text-center"
    >
      <div className="bg-base-200 text-base-content/50 rounded-full p-4">
        <Icon name={icon} className="size-8" />
      </div>
      <p className="font-semibold">{title}</p>
      {hint && <p className="text-base-content/60 max-w-sm text-sm">{hint}</p>}
      {action}
    </motion.div>
  );
}

export function OnlineBadge({ online }: { online: boolean }) {
  const { t } = useTranslation();
  return (
    <span className="inline-flex items-center gap-2 text-sm">
      <span className="relative flex size-2.5">
        {online && <span className="bg-success absolute inline-flex size-full animate-ping rounded-full opacity-60" />}
        <span className={`relative inline-flex size-2.5 rounded-full ${online ? "bg-success" : "bg-base-300"}`} />
      </span>
      {online ? t("status.online") : t("status.offline")}
    </span>
  );
}

export function StatusBadge({ status, error }: { status: ServiceStatus; error?: string | null }) {
  const { t } = useTranslation();
  const cls = { LISTENING: "badge-success", FAILED: "badge-error", INACTIVE: "badge-ghost" }[status];
  const badge = <span className={`badge badge-soft badge-sm ${cls}`}>{t(`status.${status}`)}</span>;
  return error ? (
    <span className="tooltip tooltip-left" data-tip={error}>
      {badge}
    </span>
  ) : (
    badge
  );
}

export function CopyButton({ value, className = "" }: { value: string; className?: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      // Clipboard API needs a secure context; fall back to a hidden textarea.
      const ta = document.createElement("textarea");
      ta.value = value;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <button
      type="button"
      className={`btn btn-ghost btn-xs btn-square tooltip ${className}`}
      data-tip={copied ? t("common.copied") : t("common.copy")}
      onClick={copy}
      aria-label={t("common.copy")}
    >
      <Icon name={copied ? "check" : "copy"} className={`size-4 ${copied ? "text-success" : ""}`} />
    </button>
  );
}

export function CodeBlock({ value, label }: { value: string; label?: string }) {
  return (
    <div>
      {label && <div className="text-base-content/60 mb-1 text-xs font-medium uppercase tracking-wide">{label}</div>}
      <div className="bg-base-200 relative rounded-box">
        <pre className="overflow-x-auto p-3 pr-10 font-mono text-xs leading-relaxed whitespace-pre">{value}</pre>
        <CopyButton value={value} className="absolute top-2 right-2" />
      </div>
    </div>
  );
}

/** Staggered fade-in for lists and grids. */
export const stagger = {
  container: { hidden: {}, show: { transition: { staggerChildren: 0.04 } } },
  item: { hidden: { opacity: 0, y: 8 }, show: { opacity: 1, y: 0 } },
};
