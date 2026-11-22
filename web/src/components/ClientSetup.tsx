import { useTranslation } from "react-i18next";
import { useQuery } from "../lib/gql";
import type { ServerInfo } from "../lib/types";
import { CodeBlock } from "./ui";

/** Ready-to-paste commands for running a client with the given token. */
export function ClientSetup({ token }: { token: string }) {
  const { t } = useTranslation();
  const { data } = useQuery<{ serverInfo: ServerInfo }>("{ serverInfo { controlPort publicHost fingerprint } }");
  if (!data) return <span className="loading loading-spinner" />;
  const { controlPort, publicHost, fingerprint } = data.serverInfo;
  const server = `${publicHost ?? location.hostname}:${controlPort}`;

  const docker = [
    "docker run -d --name exposr-client --restart unless-stopped --network host \\",
    `  -e EXPOSR_SERVER=${server} \\`,
    `  -e EXPOSR_TOKEN=${token} \\`,
    `  -e EXPOSR_FINGERPRINT=${fingerprint} \\`,
    "  exposr-client",
  ].join("\n");
  const cli = `exposr-client --server ${server} \\\n  --token ${token} \\\n  --fingerprint ${fingerprint}`;

  return (
    <div className="space-y-4">
      <div role="alert" className="alert alert-warning alert-soft text-sm">
        {t("clients.tokenOnce")}
      </div>
      <CodeBlock label={t("clients.token")} value={token} />
      <CodeBlock label="Docker" value={docker} />
      <CodeBlock label={t("clients.binary")} value={cli} />
    </div>
  );
}
