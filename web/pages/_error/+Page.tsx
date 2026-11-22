import { useTranslation } from "react-i18next";
import { usePageContext } from "vike-react/usePageContext";

export default function Page() {
  const { t } = useTranslation();
  const { is404 } = usePageContext();
  return (
    <div className="flex flex-col items-center gap-4 py-24 text-center">
      <p className="text-primary text-6xl font-black">{is404 ? "404" : "500"}</p>
      <p className="text-base-content/70">{is404 ? t("errors.pageNotFound") : t("errors.INTERNAL")}</p>
      <a href="/" className="btn btn-primary btn-sm">
        {t("nav.dashboard")}
      </a>
    </div>
  );
}
