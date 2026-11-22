import { motion } from "motion/react";
import { useTranslation } from "react-i18next";
import { Icon, type IconName } from "../../src/components/Icon";
import { EventRow } from "../../src/components/EventRow";
import { CopyButton, Empty, Loading, OnlineBadge, PageHeader, StatusBadge, stagger } from "../../src/components/ui";
import { formatBytes, formatDuration, formatNumber } from "../../src/lib/format";
import { useQuery } from "../../src/lib/gql";
import { useLive, type Rate } from "../../src/lib/live";
import type { Client, Event, ServerInfo, Service } from "../../src/lib/types";

const QUERY = `{
  serverInfo { version controlPort publicHost fingerprint uptimeSeconds }
  clients { id name online remoteAddr }
  services { id name clientId protocol bindPort localAddr enabled }
  events(limit: 8) { id at kind subject detail actor }
}`;

interface Data {
  serverInfo: ServerInfo;
  clients: Pick<Client, "id" | "name" | "online" | "remoteAddr">[];
  services: Omit<Service, "runtime" | "createdAt">[];
  events: Event[];
}

export default function Page() {
  const { t } = useTranslation();
  const { data } = useQuery<Data>(QUERY);
  const { live, rates, history, byId } = useLive();

  if (!data) return <Loading />;
  const online = new Set(live?.onlineClientIds ?? data.clients.filter((c) => c.online).map((c) => c.id));
  const clientName = Object.fromEntries(data.clients.map((c) => [c.id, c.name]));
  const info = data.serverInfo;

  return (
    <>
      <PageHeader title={t("dashboard.title")} subtitle={t("dashboard.subtitle")} />

      <motion.div
        variants={stagger.container}
        initial="hidden"
        animate="show"
        className="grid grid-cols-2 gap-4 lg:grid-cols-4"
      >
        <Stat icon="clients" label={t("dashboard.clientsOnline")} value={`${online.size} / ${data.clients.length}`} />
        <Stat
          icon="services"
          label={t("dashboard.servicesListening")}
          value={`${live?.servicesListening ?? "–"} / ${data.services.length}`}
        />
        <Stat icon="plug" label={t("dashboard.activeConnections")} value={formatNumber(live?.activeConnections ?? 0)} />
        <Stat
          icon="activity"
          label={t("dashboard.throughput")}
          value={formatBytes(rates.total.in + rates.total.out, true)}
          sub={`${t("dashboard.total")} ${formatBytes((live?.bytesIn ?? 0) + (live?.bytesOut ?? 0))}`}
        />
      </motion.div>

      <div className="mt-6 grid gap-6 lg:grid-cols-3">
        <div className="card bg-base-100 border-base-200 border lg:col-span-2">
          <div className="card-body">
            <div className="flex items-center justify-between">
              <h2 className="card-title text-base">{t("dashboard.traffic")}</h2>
              <div className="flex gap-4 text-xs">
                <span className="text-primary flex items-center gap-1">
                  <Icon name="arrowDown" className="size-3" />
                  {formatBytes(rates.total.in, true)}
                </span>
                <span className="text-secondary flex items-center gap-1">
                  <Icon name="arrowUp" className="size-3" />
                  {formatBytes(rates.total.out, true)}
                </span>
              </div>
            </div>
            <Sparkline history={history} />
          </div>
        </div>

        <div className="card bg-base-100 border-base-200 border">
          <div className="card-body gap-3">
            <h2 className="card-title text-base">{t("dashboard.server")}</h2>
            <dl className="space-y-2 text-sm">
              <Row label={t("dashboard.controlAddress")}>
                <span className="font-mono">
                  {info.publicHost ?? location.hostname}:{info.controlPort}
                </span>
              </Row>
              <Row label={t("dashboard.uptime")}>{formatDuration(info.uptimeSeconds)}</Row>
              <Row label={t("dashboard.version")}>{info.version}</Row>
            </dl>
            <div>
              <div className="text-base-content/60 mb-1 text-xs">{t("dashboard.fingerprint")}</div>
              <div className="bg-base-200 flex items-center gap-1 rounded-field p-2">
                <code className="flex-1 truncate text-xs">{info.fingerprint}</code>
                <CopyButton value={info.fingerprint} />
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="mt-6 grid gap-6 lg:grid-cols-3">
        <div className="card bg-base-100 border-base-200 border lg:col-span-2">
          <div className="card-body">
            <div className="flex items-center justify-between">
              <h2 className="card-title text-base">{t("nav.services")}</h2>
              <a href="/services" className="link link-hover text-sm">
                {t("common.seeAll")}
              </a>
            </div>
            {data.services.length === 0 ? (
              <Empty icon="services" title={t("services.empty")} hint={t("services.emptyHint")} />
            ) : (
              <div className="overflow-x-auto">
                <table className="table table-sm">
                  <tbody>
                    {data.services.map((s) => {
                      const rt = byId(s.id);
                      const r: Rate = rates.byService[s.id] ?? { in: 0, out: 0 };
                      return (
                        <tr key={s.id}>
                          <td>
                            <div className="font-medium">{s.name}</div>
                            <div className="text-base-content/60 flex items-center gap-2 text-xs">
                              <OnlineBadge online={online.has(s.clientId)} />
                              {clientName[s.clientId]}
                            </div>
                          </td>
                          <td className="font-mono text-xs">
                            :{s.bindPort}/{s.protocol.toLowerCase()}
                          </td>
                          <td>{rt && <StatusBadge status={rt.status} error={rt.error} />}</td>
                          <td className="text-right text-xs tabular-nums">{formatBytes(r.in + r.out, true)}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </div>

        <div className="card bg-base-100 border-base-200 border">
          <div className="card-body">
            <div className="flex items-center justify-between">
              <h2 className="card-title text-base">{t("nav.activity")}</h2>
              <a href="/activity" className="link link-hover text-sm">
                {t("common.seeAll")}
              </a>
            </div>
            {data.events.length === 0 ? (
              <p className="text-base-content/60 text-sm">{t("activity.empty")}</p>
            ) : (
              <ul className="space-y-3">
                {data.events.map((e) => (
                  <EventRow key={e.id} event={e} compact />
                ))}
              </ul>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

function Stat({ icon, label, value, sub }: { icon: IconName; label: string; value: string; sub?: string }) {
  return (
    <motion.div variants={stagger.item} className="card bg-base-100 border-base-200 border">
      <div className="card-body gap-1 p-4 sm:p-5">
        <div className="text-base-content/60 flex items-center gap-2 text-xs font-medium sm:text-sm">
          <Icon name={icon} className="size-4" />
          {label}
        </div>
        <motion.div
          key={value}
          initial={{ opacity: 0.4 }}
          animate={{ opacity: 1 }}
          className="text-xl font-bold tabular-nums sm:text-2xl"
        >
          {value}
        </motion.div>
        {sub && <div className="text-base-content/50 text-xs">{sub}</div>}
      </div>
    </motion.div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex justify-between gap-4">
      <dt className="text-base-content/60">{label}</dt>
      <dd className="truncate text-right">{children}</dd>
    </div>
  );
}

/** Last 2 minutes of throughput (in + out stacked lines). */
function Sparkline({ history }: { history: Rate[] }) {
  const W = 600;
  const H = 140;
  const points = Array.from({ length: 60 }, (_, i) => history[history.length - 60 + i] ?? { in: 0, out: 0 });
  const max = Math.max(1024, ...points.map((p) => Math.max(p.in, p.out)));
  const line = (f: (r: Rate) => number) =>
    points.map((p, i) => `${(i / 59) * W},${H - 4 - (f(p) / max) * (H - 8)}`).join(" ");
  const area = (f: (r: Rate) => number) => `0,${H} ${line(f)} ${W},${H}`;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-36 w-full" preserveAspectRatio="none">
      <polygon points={area((r) => r.in)} className="fill-primary/15" />
      <polyline points={line((r) => r.in)} className="stroke-primary fill-none" strokeWidth={2} vectorEffect="non-scaling-stroke" />
      <polygon points={area((r) => r.out)} className="fill-secondary/10" />
      <polyline points={line((r) => r.out)} className="stroke-secondary fill-none" strokeWidth={2} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
