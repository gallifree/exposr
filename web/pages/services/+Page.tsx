import { AnimatePresence, motion } from "motion/react";
import { useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../src/components/Icon";
import { ConfirmModal, Modal } from "../../src/components/Modal";
import { useToast } from "../../src/components/Toast";
import { CopyButton, Empty, ErrorAlert, Loading, OnlineBadge, PageHeader, StatusBadge } from "../../src/components/ui";
import { formatBytes, formatNumber } from "../../src/lib/format";
import { gql, GqlError, useQuery } from "../../src/lib/gql";
import { useLive } from "../../src/lib/live";
import type { Protocol, ServerInfo, Service } from "../../src/lib/types";

const QUERY = `{
  services { id name clientId protocol bindPort localAddr enabled createdAt
    runtime { id status error activeConnections totalConnections bytesIn bytesOut } }
  clients { id name online }
  serverInfo { publicHost portRangeStart portRangeEnd }
}`;

interface Data {
  services: Service[];
  clients: { id: string; name: string; online: boolean }[];
  serverInfo: Pick<ServerInfo, "publicHost" | "portRangeStart" | "portRangeEnd">;
}

export default function Page() {
  const { t } = useTranslation();
  const toast = useToast();
  const { data, error, reload } = useQuery<Data>(QUERY);
  const { live, rates, byId } = useLive();
  const [editing, setEditing] = useState<Service | "new" | null>(null);
  const [deleting, setDeleting] = useState<Service | null>(null);
  const [busy, setBusy] = useState(false);
  const [search, setSearch] = useState("");

  const clientName = useMemo(() => Object.fromEntries((data?.clients ?? []).map((c) => [c.id, c.name])), [data]);
  const online = new Set(live?.onlineClientIds ?? data?.clients.filter((c) => c.online).map((c) => c.id));
  const host = data?.serverInfo.publicHost ?? (typeof location !== "undefined" ? location.hostname : "");

  const services = (data?.services ?? []).filter((s) => {
    const q = search.trim().toLowerCase();
    return !q || [s.name, clientName[s.clientId], String(s.bindPort), s.localAddr].some((v) => v?.toLowerCase().includes(q));
  });

  const toggle = async (s: Service) => {
    setBusy(true);
    try {
      await gql(`mutation($id: ID!, $e: Boolean!) { updateService(id: $id, input: { enabled: $e }) { id } }`, {
        id: s.id,
        e: !s.enabled,
      });
      await reload();
    } catch (e) {
      toast(t(`errors.${e instanceof GqlError ? e.code : "INTERNAL"}`), "error");
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      await gql(`mutation($id: ID!) { deleteService(id: $id) }`, { id: deleting!.id });
      toast(t("services.deleted", { name: deleting!.name }));
      setDeleting(null);
      await reload();
    } catch (e) {
      toast(t(`errors.${e instanceof GqlError ? e.code : "INTERNAL"}`), "error");
    } finally {
      setBusy(false);
    }
  };

  const noClients = !!data && data.clients.length === 0;

  return (
    <>
      <PageHeader
        title={t("services.title")}
        subtitle={t("services.subtitle")}
        actions={
          <button className="btn btn-primary btn-sm" onClick={() => setEditing("new")} disabled={!data || noClients}>
            <Icon name="plus" className="size-4" />
            {t("services.new")}
          </button>
        }
      />
      <ErrorAlert code={error} />
      {!data ? (
        !error && <Loading />
      ) : noClients ? (
        <Empty
          icon="clients"
          title={t("services.needClient")}
          hint={t("services.needClientHint")}
          action={
            <a href="/clients" className="btn btn-primary btn-sm">
              {t("clients.new")}
            </a>
          }
        />
      ) : data.services.length === 0 ? (
        <Empty
          icon="services"
          title={t("services.empty")}
          hint={t("services.emptyHint")}
          action={
            <button className="btn btn-primary btn-sm" onClick={() => setEditing("new")}>
              {t("services.new")}
            </button>
          }
        />
      ) : (
        <div className="card bg-base-100 border-base-200 border">
          <div className="border-base-200 border-b p-3">
            <label className="input input-sm w-full max-w-xs">
              <Icon name="search" className="size-4 opacity-50" />
              <input placeholder={t("common.search")} value={search} onChange={(e) => setSearch(e.target.value)} />
            </label>
          </div>
          <div className="overflow-x-auto">
            <table className="table">
              <thead>
                <tr>
                  <th>{t("common.name")}</th>
                  <th>{t("services.route")}</th>
                  <th>{t("services.status")}</th>
                  <th className="text-right">{t("services.connections")}</th>
                  <th className="text-right">{t("services.traffic")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                <AnimatePresence initial={false}>
                  {services.map((s) => {
                    const rt = byId(s.id) ?? s.runtime;
                    const r = rates.byService[s.id];
                    const publicAddr = `${host}:${s.bindPort}`;
                    return (
                      <motion.tr
                        key={s.id}
                        layout
                        initial={{ opacity: 0 }}
                        animate={{ opacity: s.enabled ? 1 : 0.55 }}
                        exit={{ opacity: 0 }}
                      >
                        <td>
                          <div className="flex items-center gap-2 font-medium">
                            {s.name}
                            <span className={`badge badge-xs ${s.protocol === "TCP" ? "badge-primary" : "badge-secondary"} badge-outline`}>
                              {s.protocol}
                            </span>
                          </div>
                          <div className="text-base-content/60 flex items-center gap-2 text-xs">
                            <OnlineBadge online={online.has(s.clientId)} />· {clientName[s.clientId]}
                          </div>
                        </td>
                        <td className="font-mono text-xs">
                          <div className="flex items-center gap-1 whitespace-nowrap">
                            {publicAddr}
                            <CopyButton value={publicAddr} />
                          </div>
                          <div className="text-base-content/60 whitespace-nowrap">→ {s.localAddr}</div>
                        </td>
                        <td>
                          <StatusBadge status={rt.status} error={rt.error} />
                        </td>
                        <td className="text-right tabular-nums">
                          <div>{formatNumber(rt.activeConnections)}</div>
                          {s.protocol === "TCP" && (
                            <div className="text-base-content/50 text-xs">
                              {t("services.totalShort", { count: rt.totalConnections })}
                            </div>
                          )}
                        </td>
                        <td className="text-right text-xs tabular-nums whitespace-nowrap">
                          <div>
                            ↓ {formatBytes(rt.bytesIn)} · ↑ {formatBytes(rt.bytesOut)}
                          </div>
                          <div className="text-base-content/50">{formatBytes((r?.in ?? 0) + (r?.out ?? 0), true)}</div>
                        </td>
                        <td>
                          <div className="flex items-center justify-end gap-1">
                            <input
                              type="checkbox"
                              className="toggle toggle-success toggle-sm"
                              checked={s.enabled}
                              disabled={busy}
                              onChange={() => toggle(s)}
                              aria-label={t("common.enabled")}
                            />
                            <button className="btn btn-ghost btn-xs btn-square" onClick={() => setEditing(s)} aria-label={t("common.edit")}>
                              <Icon name="edit" className="size-4" />
                            </button>
                            <button
                              className="btn btn-ghost btn-xs btn-square text-error"
                              onClick={() => setDeleting(s)}
                              aria-label={t("common.delete")}
                            >
                              <Icon name="trash" className="size-4" />
                            </button>
                          </div>
                        </td>
                      </motion.tr>
                    );
                  })}
                </AnimatePresence>
              </tbody>
            </table>
          </div>
        </div>
      )}

      {data && (
        <ServiceForm
          service={editing}
          clients={data.clients}
          range={data.serverInfo.portRangeStart != null ? [data.serverInfo.portRangeStart, data.serverInfo.portRangeEnd!] : null}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            toast(t("common.saved"));
            reload();
          }}
        />
      )}
      <ConfirmModal
        open={!!deleting}
        onClose={() => setDeleting(null)}
        onConfirm={remove}
        busy={busy}
        title={t("services.deleteTitle")}
        message={t("services.deleteConfirm", { name: deleting?.name })}
      />
    </>
  );
}

interface FormState {
  name: string;
  clientId: string;
  protocol: Protocol;
  bindPort: string;
  localAddr: string;
  enabled: boolean;
}

function ServiceForm({
  service,
  clients,
  range,
  onClose,
  onSaved,
}: {
  service: Service | "new" | null;
  clients: { id: string; name: string }[];
  range: [number, number] | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [form, setForm] = useState<FormState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [last, setLast] = useState(service);

  if (service !== last) {
    setLast(service);
    setError(null);
    setForm(
      service === "new"
        ? { name: "", clientId: clients[0]?.id ?? "", protocol: "TCP", bindPort: "", localAddr: "127.0.0.1:", enabled: true }
        : service
          ? { ...service, bindPort: String(service.bindPort) }
          : null,
    );
  }

  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => setForm((f) => f && { ...f, [k]: v });

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!form) return;
    setBusy(true);
    setError(null);
    const input = {
      name: form.name,
      clientId: form.clientId,
      protocol: form.protocol,
      bindPort: Number(form.bindPort),
      localAddr: form.localAddr,
      enabled: form.enabled,
    };
    try {
      if (service === "new") {
        await gql(`mutation($i: CreateServiceInput!) { createService(input: $i) { id } }`, { i: input });
      } else if (service) {
        await gql(`mutation($id: ID!, $i: UpdateServiceInput!) { updateService(id: $id, input: $i) { id } }`, {
          id: service.id,
          i: input,
        });
      }
      onSaved();
    } catch (e) {
      setError(e instanceof GqlError ? e.code : "INTERNAL");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal open={!!service} onClose={onClose} title={service === "new" ? t("services.new") : t("services.edit")}>
      {form && (
        <form onSubmit={submit} className="space-y-1">
          <ErrorAlert code={error} />
          <fieldset className="fieldset">
            <legend className="fieldset-legend">{t("common.name")}</legend>
            <input
              className="input w-full"
              autoFocus
              required
              maxLength={64}
              value={form.name}
              placeholder="minecraft"
              onChange={(e) => set("name", e.target.value)}
            />
          </fieldset>
          <fieldset className="fieldset">
            <legend className="fieldset-legend">{t("services.client")}</legend>
            <select className="select w-full" value={form.clientId} onChange={(e) => set("clientId", e.target.value)}>
              {clients.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </fieldset>
          <div className="grid grid-cols-2 gap-3">
            <fieldset className="fieldset">
              <legend className="fieldset-legend">{t("services.protocol")}</legend>
              <div className="join w-full">
                {(["TCP", "UDP"] as const).map((p) => (
                  <button
                    key={p}
                    type="button"
                    className={`btn join-item flex-1 ${form.protocol === p ? "btn-primary" : ""}`}
                    onClick={() => set("protocol", p)}
                  >
                    {p}
                  </button>
                ))}
              </div>
            </fieldset>
            <fieldset className="fieldset">
              <legend className="fieldset-legend">{t("services.publicPort")}</legend>
              <input
                className="input w-full"
                type="number"
                min={range?.[0] ?? 1}
                max={range?.[1] ?? 65535}
                required
                value={form.bindPort}
                placeholder={String(range?.[0] ?? 25565)}
                onChange={(e) => set("bindPort", e.target.value)}
              />
              {range && <p className="label">{t("services.portRange", { start: range[0], end: range[1] })}</p>}
            </fieldset>
          </div>
          <fieldset className="fieldset">
            <legend className="fieldset-legend">{t("services.localAddr")}</legend>
            <input
              className="input w-full font-mono"
              required
              value={form.localAddr}
              placeholder="127.0.0.1:25565"
              onChange={(e) => set("localAddr", e.target.value)}
            />
            <p className="label whitespace-normal">{t("services.localAddrHint")}</p>
          </fieldset>
          <label className="label mt-2 cursor-pointer gap-3">
            <input
              type="checkbox"
              className="toggle toggle-success toggle-sm"
              checked={form.enabled}
              onChange={(e) => set("enabled", e.target.checked)}
            />
            {t("common.enabled")}
          </label>
          <div className="modal-action">
            <button type="button" className="btn btn-ghost" onClick={onClose}>
              {t("common.cancel")}
            </button>
            <button className="btn btn-primary" disabled={busy}>
              {busy && <span className="loading loading-spinner loading-sm" />}
              {service === "new" ? t("common.create") : t("common.save")}
            </button>
          </div>
        </form>
      )}
    </Modal>
  );
}
