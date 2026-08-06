import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Pure TypeScript logic only, so no DOM is needed. Collects *.test.ts.
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
