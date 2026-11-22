import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { EventRow } from "../../src/components/EventRow";
import { Empty, ErrorAlert, Loading, PageHeader, stagger } from "../../src/components/ui";
import { useQuery } from "../../src/lib/gql";
import type { Event } from "../../src/lib/types";

const FILTERS = ["all", "client", "service", "auth", "user"] as const;

export default function Page() {
  const { t } = useTranslation();
  const [limit, setLimit] = useState(100);
  const [filter, setFilter] = useState<(typeof FILTERS)[number]>("all");
  const { data, error, reload } = useQuery<{ events: Event[] }>(
    `query($l: Int!) { events(limit: $l) { id at kind subject detail actor } }`,
    { l: limit },
    [limit],
  );

  useEffect(() => {
    const id = setInterval(reload, 10_000);
    return () => clearInterval(id);
  }, [reload]);

  const events = (data?.events ?? []).filter((e) => filter === "all" || e.kind.startsWith(filter + "."));

  return (
    <>
      <PageHeader title={t("activity.title")} subtitle={t("activity.subtitle")} />
      <div role="tablist" className="tabs tabs-box mb-4 w-fit">
        {FILTERS.map((f) => (
          <button key={f} role="tab" className={`tab ${filter === f ? "tab-active" : ""}`} onClick={() => setFilter(f)}>
            {t(`activity.filter.${f}`)}
          </button>
        ))}
      </div>
      <ErrorAlert code={error} />
      {!data ? (
        !error && <Loading />
      ) : events.length === 0 ? (
        <Empty icon="activity" title={t("activity.empty")} />
      ) : (
        <div className="card bg-base-100 border-base-200 border">
          <motion.ul variants={stagger.container} initial="hidden" animate="show" className="card-body gap-4">
            {events.map((e) => (
              <motion.div key={e.id} variants={stagger.item}>
                <EventRow event={e} />
              </motion.div>
            ))}
          </motion.ul>
        </div>
      )}
      {data && data.events.length >= limit && (
        <div className="mt-4 flex justify-center">
          <button className="btn btn-ghost btn-sm" onClick={() => setLimit((l) => Math.min(l + 100, 500))}>
            {t("common.loadMore")}
          </button>
        </div>
      )}
    </>
  );
}
