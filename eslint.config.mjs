import js from "@eslint/js";
import tseslint from "typescript-eslint";
import globals from "globals";

export default tseslint.config(
  { ignores: ["**/dist/**", "**/node_modules/**"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["web/**/*.{ts,tsx}"],
    languageOptions: { globals: globals.browser },
    rules: { "@typescript-eslint/no-explicit-any": "error" },
  },
  {
    files: ["web/**/*.{ts,tsx}"],
    ignores: ["web/packages/api-client/**"],
    rules: {
      "no-restricted-globals": [
        "error",
        { name: "fetch", message: "Use @gfa/api-client." },
        { name: "WebSocket", message: "Use @gfa/api-client." },
        { name: "XMLHttpRequest", message: "Use @gfa/api-client." },
      ],
    },
  },
);
