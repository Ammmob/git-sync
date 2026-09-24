import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  timeout: 30000,
  fullyParallel: true,
  use: {
    baseURL: "http://127.0.0.1:1420",
    headless: true,
    viewport: { width: 1180, height: 800 },
  },
  webServer: {
    command: "npm run dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: false,
    env: { VITE_UI_PREVIEW: "true" },
  },
  reporter: "list",
});
