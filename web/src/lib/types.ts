export type Protocol = "TCP" | "UDP" | "HTTP";
export type ServiceStatus = "INACTIVE" | "LISTENING" | "FAILED";

export interface User {
  id: string;
  username: string;
  createdAt: string;
  lastLoginAt: string | null;
}

export interface Client {
  id: string;
  name: string;
  enabled: boolean;
  createdAt: string;
  lastSeenAt: string | null;
  online: boolean;
  connectedAt: string | null;
  remoteAddr: string | null;
  clientVersion: string | null;
  services: { id: string }[];
}

export interface ServiceLive {
  id: string;
  status: ServiceStatus;
  error: string | null;
  activeConnections: number;
  totalConnections: number;
  bytesIn: number;
  bytesOut: number;
}

export interface Service {
  id: string;
  name: string;
  clientId: string;
  protocol: Protocol;
  bindPort: number;
  localAddr: string;
  enabled: boolean;
  createdAt: string;
  runtime: ServiceLive;
}

export interface Live {
  onlineClientIds: string[];
  services: ServiceLive[];
  clientsTotal: number;
  servicesTotal: number;
  servicesListening: number;
  activeConnections: number;
  bytesIn: number;
  bytesOut: number;
}

export interface ServerInfo {
  version: string;
  controlPort: number;
  publicHost: string | null;
  fingerprint: string;
  uptimeSeconds: number;
  portRangeStart: number | null;
  portRangeEnd: number | null;
}

export interface Event {
  id: string;
  at: string;
  kind: string;
  subject: string;
  detail: string | null;
  actor: string | null;
}

export const LIVE_FIELDS = `
  onlineClientIds clientsTotal servicesTotal servicesListening activeConnections bytesIn bytesOut
  services { id status error activeConnections totalConnections bytesIn bytesOut }
`;
export const LIVE_SUBSCRIPTION = `subscription { live(intervalMs: 2000) { ${LIVE_FIELDS} } }`;
