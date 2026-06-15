import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // 순수 TS 로직만 테스트(DOM 불필요). *.test.ts 만 수집.
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
