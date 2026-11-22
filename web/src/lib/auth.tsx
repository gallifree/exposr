import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { gql, resetSubscriptions, UNAUTHENTICATED_EVENT } from "./gql";
import type { User } from "./types";

interface AuthState {
  /** `undefined` while loading, `null` when signed out. */
  me: User | null | undefined;
  setMe: (u: User | null) => void;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthState>(null!);

export const USER_FIELDS = "id username createdAt lastLoginAt";

export function AuthProvider({ children }: { children: ReactNode }) {
  const [me, setMe] = useState<User | null | undefined>(undefined);

  useEffect(() => {
    gql<{ me: User | null }>(`{ me { ${USER_FIELDS} } }`)
      .then((d) => setMe(d.me))
      .catch(() => setMe(null));
    const onUnauth = () => setMe(null);
    window.addEventListener(UNAUTHENTICATED_EVENT, onUnauth);
    return () => window.removeEventListener(UNAUTHENTICATED_EVENT, onUnauth);
  }, []);

  const logout = useCallback(async () => {
    await gql("mutation { logout }").catch(() => {});
    resetSubscriptions();
    setMe(null);
  }, []);

  const set = useCallback((u: User | null) => {
    resetSubscriptions();
    setMe(u);
  }, []);

  return <AuthContext.Provider value={{ me, setMe: set, logout }}>{children}</AuthContext.Provider>;
}

export const useAuth = () => useContext(AuthContext);
