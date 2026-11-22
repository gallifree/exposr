import type { Config } from "vike/types";
import vikeReact from "vike-react/config";

// Static admin UI: every page is prerendered to an HTML shell (no SSR — data
// is fetched client-side from the GraphQL API) and served by exposr-server.
export default {
  extends: [vikeReact],
  prerender: true,
  ssr: false,
  title: "Exposr",
  lang: "en",
} satisfies Config;
