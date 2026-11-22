import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { languages } from "../i18n";
import { Icon } from "./Icon";

export function LanguageSwitch() {
  const { i18n } = useTranslation();
  return (
    <div className="join">
      {languages.map((l) => (
        <button
          key={l.code}
          className={`btn btn-xs join-item ${i18n.resolvedLanguage === l.code ? "btn-primary" : "btn-ghost"}`}
          onClick={() => i18n.changeLanguage(l.code)}
          title={l.label}
        >
          {l.code.toUpperCase()}
        </button>
      ))}
    </div>
  );
}

export function ThemeToggle() {
  const [dark, setDark] = useState(false);
  useEffect(() => {
    const explicit = document.documentElement.dataset.theme;
    setDark(explicit ? explicit === "dark" : matchMedia("(prefers-color-scheme: dark)").matches);
  }, []);
  const toggle = () => {
    const next = dark ? "light" : "dark";
    document.documentElement.dataset.theme = next;
    try {
      localStorage.setItem("theme", next);
    } catch {}
    setDark(!dark);
  };
  return (
    <button className="btn btn-ghost btn-sm btn-square" onClick={toggle} aria-label="theme">
      <motion.span key={String(dark)} initial={{ rotate: -90, opacity: 0 }} animate={{ rotate: 0, opacity: 1 }}>
        <Icon name={dark ? "moon" : "sun"} className="size-4" />
      </motion.span>
    </button>
  );
}
