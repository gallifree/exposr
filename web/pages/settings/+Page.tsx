import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../src/components/Icon";
import { ConfirmModal, Modal } from "../../src/components/Modal";
import { useToast } from "../../src/components/Toast";
import { CopyButton, ErrorAlert, Loading, PageHeader } from "../../src/components/ui";
import { USER_FIELDS, useAuth } from "../../src/lib/auth";
import { formatDuration, timeAgo } from "../../src/lib/format";
import { gql, GqlError, useQuery } from "../../src/lib/gql";
import type { ServerInfo, User } from "../../src/lib/types";

export default function Page() {
  const { t } = useTranslation();
  return (
    <>
      <PageHeader title={t("settings.title")} />
      <div className="grid gap-6 lg:grid-cols-2">
        <ChangePassword />
        <ServerCard />
        <div className="lg:col-span-2">
          <Admins />
        </div>
      </div>
    </>
  );
}

function ChangePassword() {
  const { t } = useTranslation();
  const toast = useToast();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (next !== confirm) return setError("PASSWORD_MISMATCH");
    setBusy(true);
    setError(null);
    try {
      await gql(`mutation($c: String!, $n: String!) { changePassword(current: $c, newPassword: $n) }`, { c: current, n: next });
      toast(t("settings.passwordChanged"));
      setCurrent("");
      setNext("");
      setConfirm("");
    } catch (e) {
      setError(e instanceof GqlError ? e.code : "INTERNAL");
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="card bg-base-100 border-base-200 border">
      <div className="card-body">
        <h2 className="card-title text-base">{t("settings.changePassword")}</h2>
        <ErrorAlert code={error} />
        {(
          [
            ["settings.currentPassword", current, setCurrent, "current-password"],
            ["settings.newPassword", next, setNext, "new-password"],
            ["settings.confirmPassword", confirm, setConfirm, "new-password"],
          ] as const
        ).map(([label, value, setter, ac]) => (
          <fieldset key={label} className="fieldset py-0">
            <legend className="fieldset-legend">{t(label)}</legend>
            <input type="password" className="input w-full" autoComplete={ac} required value={value} onChange={(e) => setter(e.target.value)} />
          </fieldset>
        ))}
        <div className="card-actions mt-2 justify-end">
          <button className="btn btn-primary btn-sm" disabled={busy}>
            {busy && <span className="loading loading-spinner loading-sm" />}
            {t("common.save")}
          </button>
        </div>
      </div>
    </form>
  );
}

function ServerCard() {
  const { t } = useTranslation();
  const { data } = useQuery<{ serverInfo: ServerInfo }>("{ serverInfo { version controlPort publicHost fingerprint uptimeSeconds } }");
  const info = data?.serverInfo;
  return (
    <div className="card bg-base-100 border-base-200 border">
      <div className="card-body">
        <h2 className="card-title text-base">{t("dashboard.server")}</h2>
        {!info ? (
          <Loading />
        ) : (
          <dl className="space-y-3 text-sm">
            <div className="flex justify-between">
              <dt className="text-base-content/60">{t("dashboard.version")}</dt>
              <dd>{info.version}</dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-base-content/60">{t("dashboard.uptime")}</dt>
              <dd>{formatDuration(info.uptimeSeconds)}</dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-base-content/60">{t("dashboard.controlAddress")}</dt>
              <dd className="font-mono">
                {info.publicHost ?? location.hostname}:{info.controlPort}
              </dd>
            </div>
            {!info.publicHost && <p className="text-warning text-xs">{t("settings.publicHostHint")}</p>}
            <div>
              <dt className="text-base-content/60 mb-1">{t("dashboard.fingerprint")}</dt>
              <dd className="bg-base-200 flex items-center gap-1 rounded-field p-2">
                <code className="flex-1 text-xs break-all">{info.fingerprint}</code>
                <CopyButton value={info.fingerprint} />
              </dd>
            </div>
          </dl>
        )}
      </div>
    </div>
  );
}

function Admins() {
  const { t } = useTranslation();
  const toast = useToast();
  const { me } = useAuth();
  const { data, error, reload } = useQuery<{ users: User[] }>(`{ users { ${USER_FIELDS} } }`);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<User | null>(null);
  const [form, setForm] = useState({ username: "", password: "" });
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const create = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setFormError(null);
    try {
      await gql(`mutation($u: String!, $p: String!) { createUser(username: $u, password: $p) { id } }`, {
        u: form.username,
        p: form.password,
      });
      toast(t("settings.adminCreated", { name: form.username }));
      setCreating(false);
      setForm({ username: "", password: "" });
      reload();
    } catch (e) {
      setFormError(e instanceof GqlError ? e.code : "INTERNAL");
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      await gql(`mutation($id: ID!) { deleteUser(id: $id) }`, { id: deleting!.id });
      toast(t("settings.adminDeleted", { name: deleting!.username }));
      setDeleting(null);
      reload();
    } catch (e) {
      toast(t(`errors.${e instanceof GqlError ? e.code : "INTERNAL"}`), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card bg-base-100 border-base-200 border">
      <div className="card-body">
        <div className="flex items-center justify-between">
          <h2 className="card-title text-base">{t("settings.admins")}</h2>
          <button className="btn btn-primary btn-sm" onClick={() => setCreating(true)}>
            <Icon name="plus" className="size-4" />
            {t("settings.newAdmin")}
          </button>
        </div>
        <ErrorAlert code={error} />
        {!data ? (
          !error && <Loading />
        ) : (
          <div className="overflow-x-auto">
            <table className="table">
              <thead>
                <tr>
                  <th>{t("login.username")}</th>
                  <th>{t("settings.lastLogin")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {data.users.map((u) => (
                  <tr key={u.id}>
                    <td className="font-medium">
                      {u.username}
                      {u.id === me?.id && <span className="badge badge-sm badge-ghost ml-2">{t("settings.you")}</span>}
                    </td>
                    <td>{timeAgo(u.lastLoginAt)}</td>
                    <td className="text-right">
                      {u.id !== me?.id && (
                        <button className="btn btn-ghost btn-xs btn-square text-error" onClick={() => setDeleting(u)} aria-label={t("common.delete")}>
                          <Icon name="trash" className="size-4" />
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <Modal open={creating} onClose={() => setCreating(false)} title={t("settings.newAdmin")}>
        <form onSubmit={create} className="space-y-1">
          <ErrorAlert code={formError} />
          <fieldset className="fieldset">
            <legend className="fieldset-legend">{t("login.username")}</legend>
            <input
              className="input w-full"
              autoFocus
              required
              value={form.username}
              onChange={(e) => setForm({ ...form, username: e.target.value })}
            />
          </fieldset>
          <fieldset className="fieldset">
            <legend className="fieldset-legend">{t("login.password")}</legend>
            <input
              type="password"
              className="input w-full"
              autoComplete="new-password"
              required
              minLength={8}
              value={form.password}
              onChange={(e) => setForm({ ...form, password: e.target.value })}
            />
            <p className="label">{t("settings.passwordHint")}</p>
          </fieldset>
          <div className="modal-action">
            <button type="button" className="btn btn-ghost" onClick={() => setCreating(false)}>
              {t("common.cancel")}
            </button>
            <button className="btn btn-primary" disabled={busy}>
              {busy && <span className="loading loading-spinner loading-sm" />}
              {t("common.create")}
            </button>
          </div>
        </form>
      </Modal>
      <ConfirmModal
        open={!!deleting}
        onClose={() => setDeleting(null)}
        onConfirm={remove}
        busy={busy}
        title={t("settings.deleteAdmin")}
        message={t("settings.deleteAdminConfirm", { name: deleting?.username })}
      />
    </div>
  );
}
