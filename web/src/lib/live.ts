import { useEffect, useRef, useState } from "react";
import { useSubscription } from "./gql";
import { LIVE_SUBSCRIPTION, type Live, type ServiceLive } from "./types";

export interface Rate {
  in: number;
  out: number;
}

/** Live stats pushed by the server, plus per-service and total byte rates. */
export function useLive() {
  const live = useSubscription<{ live: Live }>(LIVE_SUBSCRIPTION)?.live ?? null;
  const prev = useRef<{ at: number; live: Live } | null>(null);
  const [rates, setRates] = useState<{ total: Rate; byService: Record<string, Rate> }>({ total: { in: 0, out: 0 }, byService: {} });
  const [history, setHistory] = useState<Rate[]>([]);

  useEffect(() => {
    if (!live) return;
    const now = Date.now();
    const p = prev.current;
    if (p) {
      const dt = Math.max((now - p.at) / 1000, 0.001);
      const before = new Map(p.live.services.map((s) => [s.id, s]));
      const byService: Record<string, Rate> = {};
      for (const s of live.services) {
        const b = before.get(s.id);
        byService[s.id] = b
          ? { in: Math.max(0, s.bytesIn - b.bytesIn) / dt, out: Math.max(0, s.bytesOut - b.bytesOut) / dt }
          : { in: 0, out: 0 };
      }
      const total = Object.values(byService).reduce((a, r) => ({ in: a.in + r.in, out: a.out + r.out }), { in: 0, out: 0 });
      setRates({ total, byService });
      setHistory((h) => [...h.slice(-59), total]);
    }
    prev.current = { at: now, live };
  }, [live]);

  const byId = (id: string): ServiceLive | undefined => live?.services.find((s) => s.id === id);
  return { live, rates, history, byId };
}
