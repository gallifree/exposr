import { motion } from "motion/react";
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../src/components/Icon";
import { ErrorAlert } from "../../src/components/ui";
import { USER_FIELDS, useAuth } from "../../src/lib/auth";
import { gql, GqlError } from "../../src/lib/gql";
import type { User } from "../../src/lib/types";
import { LanguageSwitch, ThemeToggle } from "../../src/components/Prefs";

export default function Page() {
  const { t } = useTranslation();
  const { setMe } = useAuth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const d = await gql<{ login: User }>(
        `mutation($u: String!, $p: String!) { login(username: $u, password: $p) { ${USER_FIELDS} } }`,
        { u: username, p: password },
      );
      setMe(d.login);
    } catch (e) {
      setError(e instanceof GqlError ? e.code : "INTERNAL");
      setBusy(false);
    }
  };

  return (
    <div className="bg-base-200 relative flex min-h-screen items-center justify-center overflow-hidden p-4">
      <motion.div
        className="bg-primary/20 pointer-events-none absolute -top-40 -left-40 size-[28rem] rounded-full blur-3xl"
        animate={{ x: [0, 40, 0], y: [0, 30, 0] }}
        transition={{ duration: 14, repeat: Infinity, ease: "easeInOut" }}
      />
      <motion.div
        className="bg-secondary/20 pointer-events-none absolute -right-40 -bottom-40 size-[28rem] rounded-full blur-3xl"
        animate={{ x: [0, -40, 0], y: [0, -30, 0] }}
        transition={{ duration: 16, repeat: Infinity, ease: "easeInOut" }}
      />
      <div className="absolute top-4 right-4 flex items-center gap-2">
        <LanguageSwitch />
        <ThemeToggle />
      </div>
      <motion.form
        onSubmit={submit}
        initial={{ opacity: 0, y: 20, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ type: "spring", stiffness: 260, damping: 26 }}
        className="card bg-base-100 relative w-full max-w-sm shadow-xl"
      >
        <div className="card-body gap-4">
          <div className="mb-2 flex flex-col items-center gap-3 text-center">
            <span className="bg-primary text-primary-content grid size-12 place-items-center rounded-2xl">
              <Icon name="activity" className="size-7" />
            </span>
            <h1 className="text-2xl font-bold">{t("login.title")}</h1>
            <p className="text-base-content/60 text-sm">{t("login.subtitle")}</p>
          </div>
          <ErrorAlert code={error} />
          <label className="input w-full">
            <Icon name="user" className="size-4 opacity-50" />
            <input
              autoFocus
              autoComplete="username"
              placeholder={t("login.username")}
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
            />
          </label>
          <label className="input w-full">
            <Icon name="lock" className="size-4 opacity-50" />
            <input
              type="password"
              autoComplete="current-password"
              placeholder={t("login.password")}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
          </label>
          <button className="btn btn-primary mt-2 w-full" disabled={busy}>
            {busy && <span className="loading loading-spinner loading-sm" />}
            {t("login.submit")}
          </button>
        </div>
      </motion.form>
    </div>
  );
}
