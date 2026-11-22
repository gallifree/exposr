import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import vike from "vike/plugin";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [vike(), react(), tailwindcss()],
  server: {
    // `npm run dev` talks to a exposr-server running locally.
    proxy: { "/api": { target: "http://localhost:8080", ws: true } },
  },
});
