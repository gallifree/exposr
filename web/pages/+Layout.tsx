import "../src/style.css";
import "../src/i18n";

import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { navigate } from "vike/client/router";
import { usePageContext } from "vike-react/usePageContext";
import { Icon, type IconName } from "../src/components/Icon";
import { ToastProvider } from "../src/components/Toast";
import { Loading } from "../src/components/ui";
import { LanguageSwitch, ThemeToggle } from "../src/components/Prefs";
import { AuthProvider, useAuth } from "../src/lib/auth";

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <AuthProvider>
      <ToastProvider>
        <Guard>{children}</Guard>
      </ToastProvider>
    </AuthProvider>
  );
}

const NAV: { href: string; key: string; icon: IconName }[] = [
  { href: "/", key: "nav.dashboard", icon: "dashboard" },
  { href: "/clients", key: "nav.clients", icon: "clients" },
  { href: "/services", key: "nav.services", icon: "services" },
  { href: "/activity", key: "nav.activity", icon: "activity" },
  { href: "/settings", key: "nav.settings", icon: "settings" },
];

function normalize(path: string) {
  return path.length > 1 ? path.replace(/\/$/, "") : path;
}

/** Sends signed-out visitors to /login and signed-in ones away from it. */
function Guard({ children }: { children: ReactNode }) {
  const { me } = useAuth();
  const path = normalize(usePageContext().urlPathname);
  const isLogin = path === "/login";

  useEffect(() => {
    if (me === null && !isLogin) navigate(`/login`);
    if (me && isLogin) navigate("/");
  }, [me, isLogin]);

  if (isLogin) return <>{me === null ? children : <Loading />}</>;
  if (!me) return <Loading />;
  return <Shell path={path}>{children}</Shell>;
}

function Shell({ path, children }: { path: string; children: ReactNode }) {
  const { t } = useTranslation();
  const { me, logout } = useAuth();
  const [drawer, setDrawer] = useState(false);

  useEffect(() => setDrawer(false), [path]);

  return (
    <div className="drawer lg:drawer-open">
      <input id="nav" type="checkbox" className="drawer-toggle" checked={drawer} onChange={(e) => setDrawer(e.target.checked)} />
      <div className="drawer-content flex min-h-screen flex-col">
        <header className="navbar bg-base-100/80 border-base-200 sticky top-0 z-30 border-b backdrop-blur lg:hidden">
          <label htmlFor="nav" className="btn btn-ghost btn-square" aria-label="menu">
            <Icon name="menu" />
          </label>
          <Brand />
        </header>
        <main className="mx-auto w-full max-w-6xl flex-1 px-4 py-6 sm:px-6 lg:py-10">
          <AnimatePresence mode="wait">
            <motion.div
              key={path}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -6 }}
              transition={{ duration: 0.18 }}
            >
              {children}
            </motion.div>
          </AnimatePresence>
        </main>
      </div>
      <div className="drawer-side z-40">
        <label htmlFor="nav" className="drawer-overlay" aria-label="close" />
        <aside className="bg-base-200 flex min-h-full w-64 flex-col p-4">
          <div className="mb-6 px-2 pt-1">
            <Brand />
          </div>
          <ul className="menu w-full gap-1 p-0">
            {NAV.map((n) => {
              const active = n.href === "/" ? path === "/" : path.startsWith(n.href);
              return (
                <li key={n.href}>
                  <a href={n.href} className={`relative ${active ? "menu-active" : ""}`}>
                    <Icon name={n.icon} />
                    {t(n.key)}
                  </a>
                </li>
              );
            })}
          </ul>
          <div className="mt-auto space-y-3 pt-6">
            <div className="flex items-center justify-between px-2">
              <LanguageSwitch />
              <ThemeToggle />
            </div>
            <div className="bg-base-100 flex items-center gap-3 rounded-box p-3">
              <div className="avatar avatar-placeholder">
                <div className="bg-primary text-primary-content w-9 rounded-full">
                  <span className="text-sm font-semibold">{me?.username.slice(0, 2).toUpperCase()}</span>
                </div>
              </div>
              <span className="flex-1 truncate text-sm font-medium">{me?.username}</span>
              <button className="btn btn-ghost btn-sm btn-square tooltip" data-tip={t("nav.logout")} onClick={logout}>
                <Icon name="logout" className="size-4" />
              </button>
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
}

function Brand() {
  return (
    <a href="/" className="flex items-center gap-2 text-lg font-bold">
      <span className="bg-primary text-primary-content grid size-8 place-items-center rounded-lg">
        <Icon name="activity" className="size-5" />
      </span>
      Exposr
    </a>
  );
}
