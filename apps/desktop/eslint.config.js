import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default [
  { ignores: ["dist", "src/api/generated.ts"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  { files: ["**/*.{ts,tsx}"], rules: { "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }] } },
];
