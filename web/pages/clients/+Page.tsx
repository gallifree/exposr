import { AnimatePresence, motion } from "motion/react";
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { ClientSetup } from "../../src/components/ClientSetup";
import { Icon } from "../../src/components/Icon";
import { ConfirmModal, Modal } from "../../src/components/Modal";
import { useToast } from "../../src/components/Toast";
import { Empty, ErrorAlert, Loading, OnlineBadge, PageHeader } from "../../src/components/ui";
import { formatDate, timeAgo } from "../../src/lib/format";
import { gql, GqlError, useQuery } from "../../src/lib/gql";
import { useLive } from "../../src/lib/live";
import type { Client } from "../../src/lib/types";

const FIELDS = "id name enabled createdAt lastSeenAt online connectedAt remoteAddr clientVersion services { id }";

export default function Page() {
  const { t } = useTranslation();
  const toast = useToast();
  const { data, error, reload } = useQuery<{ clients: Client[] }>(`{ clients { ${FIELDS} } }`);
  const { live } = useLive();
  const [editing, setEditing] = useState<Client | "new" | null>(null);
  const [deleting, setDeleting] = useState<Client | null>(null);
  const [regen, setRegen] = useState<Client | null>(null);
  const [token, setToken] = useState<{ name: string; token: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const online = new Set(live?.onlineClientIds ?? data?.clients.filter((c) => c.online).map((c) => c.id));

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
      await reload();
    } catch (e) {
      toast(t(`errors.${e instanceof GqlError ? e.code : "INTERNAL"}`), "error");
    } finally {
      setBusy(false);
    }
  };

  const toggle = (c: Client) =>
    run(async () => {
      await gql(`mutation($id: ID!, $e: Boolean!) { updateClient(id: $id, input: { enabled: $e }) { id } }`, {
        id: c.id,
        e: !c.enabled,
      });
      toast(t(c.enabled ? "clients.disabled" : "clients.enabled", { name: c.name }));
    });

  const remove = () =>
    run(async () => {
      await gql(`mutation($id: ID!) { deleteClient(id: $id) }`, { id: deleting!.id });
      toast(t("clients.deleted", { name: deleting!.name }));
      setDeleting(null);
    });

  const regenerate = () =>
    run(async () => {
      const d = await gql<{ regenerateClientToken: { token: string } }>(
        `mutation($id: ID!) { regenerateClientToken(id: $id) { token } }`,
        { id: regen!.id },
      );
      setToken({ name: regen!.name, token: d.regenerateClientToken.token });
      setRegen(null);
    });

  return (
    <>
      <PageHeader
        title={t("clients.title")}
        subtitle={t("clients.subtitle")}
        actions={
          <button className="btn btn-primary btn-sm" onClick={() => setEditing("new")}>
            <Icon name="plus" className="size-4" />
            {t("clients.new")}
          </button>
        }
      />
      <ErrorAlert code={error} />
      {!data ? (
        !error && <Loading />
      ) : data.clients.length === 0 ? (
        <Empty
          icon="clients"
          title={t("clients.empty")}
          hint={t("clients.emptyHint")}
          action={
            <button className="btn btn-primary btn-sm" onClick={() => setEditing("new")}>
              {t("clients.new")}
            </button>
          }
        />
      ) : (
        <div className="grid gap-4 md:grid-cols-2">
          <AnimatePresence initial={false}>
            {data.clients.map((c, i) => (
              <motion.div
                key={c.id}
                layout
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0, transition: { delay: i * 0.03 } }}
                exit={{ opacity: 0, scale: 0.96 }}
                className={`card bg-base-100 border-base-200 border ${c.enabled ? "" : "opacity-60"}`}
              >
                <div className="card-body gap-3 p-5">
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <h2 className="truncate text-lg font-semibold">{c.name}</h2>
                      <OnlineBadge online={online.has(c.id)} />
                    </div>
                    <input
                      type="checkbox"
                      className="toggle toggle-success toggle-sm tooltip"
                      data-tip={t(c.enabled ? "common.enabled" : "common.disabled")}
                      checked={c.enabled}
                      disabled={busy}
                      onChange={() => toggle(c)}
                    />
                  </div>
                  <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
                    <dt className="text-base-content/60">{t("clients.address")}</dt>
                    <dd className="truncate text-right font-mono text-xs leading-5">{online.has(c.id) ? c.remoteAddr ?? "—" : "—"}</dd>
                    <dt className="text-base-content/60">{t("clients.lastSeen")}</dt>
                    <dd className="text-right" title={formatDate(c.lastSeenAt)}>
                      {online.has(c.id) ? t("status.now") : timeAgo(c.lastSeenAt)}
                    </dd>
                    <dt className="text-base-content/60">{t("clients.version")}</dt>
                    <dd className="text-right">{c.clientVersion ?? "—"}</dd>
                    <dt className="text-base-content/60">{t("nav.services")}</dt>
                    <dd className="text-right">
                      <a className="link link-hover" href="/services">
                        {c.services.length}
                      </a>
                    </dd>
                  </dl>
                  <div className="card-actions border-base-200 justify-end border-t pt-3">
                    <button className="btn btn-ghost btn-xs" onClick={() => setEditing(c)}>
                      <Icon name="edit" className="size-3.5" />
                      {t("common.rename")}
                    </button>
                    <button className="btn btn-ghost btn-xs" onClick={() => setRegen(c)}>
                      <Icon name="key" className="size-3.5" />
                      {t("clients.newToken")}
                    </button>
                    <button className="btn btn-ghost btn-xs text-error" onClick={() => setDeleting(c)}>
                      <Icon name="trash" className="size-3.5" />
                      {t("common.delete")}
                    </button>
                  </div>
                </div>
              </motion.div>
            ))}
          </AnimatePresence>
        </div>
      )}

      <ClientForm
        client={editing}
        onClose={() => setEditing(null)}
        onSaved={(created) => {
          setEditing(null);
          reload();
          if (created) setToken(created);
          else toast(t("common.saved"));
        }}
      />

      <Modal
        open={!!token}
        onClose={() => setToken(null)}
        title={t("clients.setupTitle", { name: token?.name })}
        wide
        actions={
          <button className="btn btn-primary" onClick={() => setToken(null)}>
            {t("common.done")}
          </button>
        }
      >
        {token && <ClientSetup token={token.token} />}
      </Modal>

      <ConfirmModal
        open={!!regen}
        onClose={() => setRegen(null)}
        onConfirm={regenerate}
        busy={busy}
        title={t("clients.newToken")}
        message={t("clients.newTokenConfirm", { name: regen?.name })}
        confirmLabel={t("clients.newToken")}
      />
      <ConfirmModal
        open={!!deleting}
        onClose={() => setDeleting(null)}
        onConfirm={remove}
        busy={busy}
        title={t("clients.deleteTitle")}
        message={t("clients.deleteConfirm", { name: deleting?.name, count: deleting?.services.length ?? 0 })}
      />
    </>
  );
}

function ClientForm({
  client,
  onClose,
  onSaved,
}: {
  client: Client | "new" | null;
  onClose: () => void;
  onSaved: (created?: { name: string; token: string }) => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastClient, setLastClient] = useState(client);

  if (client !== lastClient) {
    setLastClient(client);
    setName(client && client !== "new" ? client.name : "");
    setError(null);
  }

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (client === "new") {
        const d = await gql<{ createClient: { token: string; client: { name: string } } }>(
          `mutation($n: String!) { createClient(name: $n) { token client { name } } }`,
          { n: name },
        );
        onSaved({ name: d.createClient.client.name, token: d.createClient.token });
      } else if (client) {
        await gql(`mutation($id: ID!, $n: String!) { updateClient(id: $id, input: { name: $n }) { id } }`, {
          id: client.id,
          n: name,
        });
        onSaved();
      }
    } catch (e) {
      setError(e instanceof GqlError ? e.code : "INTERNAL");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal open={!!client} onClose={onClose} title={client === "new" ? t("clients.new") : t("common.rename")}>
      <form onSubmit={submit} className="space-y-4">
        <ErrorAlert code={error} />
        <fieldset className="fieldset">
          <legend className="fieldset-legend">{t("common.name")}</legend>
          <input
            className="input w-full"
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t("clients.namePlaceholder")}
            required
            maxLength={64}
          />
          <p className="label">{t("common.nameHint")}</p>
        </fieldset>
        <div className="modal-action">
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button className="btn btn-primary" disabled={busy}>
            {busy && <span className="loading loading-spinner loading-sm" />}
            {client === "new" ? t("common.create") : t("common.save")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
