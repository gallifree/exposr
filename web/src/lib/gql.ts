import { createClient, type Client } from "graphql-ws";
import { useCallback, useEffect, useRef, useState } from "react";

/** A GraphQL error; `code` is a stable key translated under `errors.*`. */
export class GqlError extends Error {
  constructor(public code: string) {
    super(code);
  }
}

export const UNAUTHENTICATED_EVENT = "exposr:unauthenticated";

export async function gql<T>(query: string, variables?: Record<string, unknown>): Promise<T> {
  let res: Response;
  try {
    res = await fetch("/api/graphql", {
      method: "POST",
      headers: { "content-type": "application/json" },
      credentials: "same-origin",
      body: JSON.stringify({ query, variables }),
    });
  } catch {
    throw new GqlError("NETWORK");
  }
  const body = await res.json().catch(() => null);
  if (!body) throw new GqlError("NETWORK");
  const error = body.errors?.[0]?.message as string | undefined;
  if (error) {
    if (error === "UNAUTHENTICATED") window.dispatchEvent(new Event(UNAUTHENTICATED_EVENT));
    throw new GqlError(/^[A-Z_]+$/.test(error) ? error : "INTERNAL");
  }
  return body.data as T;
}

/** Runs a query on mount (and when `deps` change); `reload` re-runs it quietly. */
export function useQuery<T>(query: string, variables?: Record<string, unknown>, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const vars = useRef(variables);
  vars.current = variables;

  const reload = useCallback(async () => {
    try {
      setData(await gql<T>(query, vars.current));
      setError(null);
    } catch (e) {
      setError(e instanceof GqlError ? e.code : "INTERNAL");
    } finally {
      setLoading(false);
    }
  }, [query]);

  useEffect(() => {
    setLoading(true);
    reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reload, ...deps]);

  return { data, error, loading, reload, setData };
}

let wsClient: Client | null = null;

function ws(): Client {
  if (!wsClient) {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    wsClient = createClient({
      url: `${proto}://${location.host}/api/graphql/ws`,
      retryAttempts: Infinity,
      shouldRetry: () => true,
      lazy: true,
    });
  }
  return wsClient;
}

/** Drops the WebSocket (e.g. on logout, so the next one carries the new cookie). */
export function resetSubscriptions() {
  wsClient?.dispose();
  wsClient = null;
}

/** Subscribes for as long as the component is mounted. */
export function useSubscription<T>(query: string, variables?: Record<string, unknown>): T | null {
  const [data, setData] = useState<T | null>(null);
  const key = JSON.stringify(variables ?? {});
  useEffect(() => {
    return ws().subscribe<T>(
      { query, variables },
      {
        next: (msg) => msg.data && setData(msg.data as T),
        error: () => {},
        complete: () => {},
      },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, key]);
  return data;
}
