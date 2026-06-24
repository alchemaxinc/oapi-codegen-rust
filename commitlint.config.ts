import type { UserConfig } from "@commitlint/types";

const config: UserConfig = {
  extends: ["@commitlint/config-conventional"],
  ignores: [
    (commit: string): boolean => commit.toLowerCase().startsWith("merge"),
  ],
  rules: {
    "header-max-length": [2, "always", 150],
    "body-max-line-length": [2, "always", 150],
    "footer-max-line-length": [2, "always", 150],
  },
};

export default config;
