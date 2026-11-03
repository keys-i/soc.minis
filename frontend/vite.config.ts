import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "vitest/config";

export default defineConfig({
  base: "/soc/",
  plugins: [react()],
  server: {
    port: 5174,
    proxy: {
      "/soc/api": "http://127.0.0.1:8787",
    },
  },
  test: {
    environment: "jsdom",
  },
});
