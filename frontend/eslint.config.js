// Flat-config ESLint for the Kairo frontend monorepo.
//
// Stack rationale: the Kairo web-client is a strict-mode TS +
// React + TanStack project (DECISIONS.md §12). The chosen
// presets give us:
//
// - `@eslint/js` recommended for JS basics.
// - `typescript-eslint` recommended-type-checked for the TS
//   rules that catch real bugs (no-floating-promises,
//   no-misused-promises, await-thenable, no-unsafe-*). We use
//   the type-checked variant because every package opts into
//   strict mode anyway; the lint cost is amortized by Turbo
//   caching.
// - `eslint-plugin-react` + `eslint-plugin-react-hooks` for the
//   rules-of-hooks / rules-of-components contract.
// - `eslint-plugin-jsx-a11y` for the accessibility baseline
//   `WEB_CLIENT.md` §20 expects.
// - `eslint-config-prettier` *last* so it disables every rule
//   that conflicts with Prettier's formatting (we use Prettier
//   for layout, ESLint for semantics — no overlap).

import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import react from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';
import jsxA11y from 'eslint-plugin-jsx-a11y';
import prettier from 'eslint-config-prettier';
import globals from 'globals';

export default tseslint.config(
  {
    ignores: [
      '**/node_modules/**',
      '**/dist/**',
      '**/build/**',
      '**/.turbo/**',
      '**/generated/**',
      '**/public/mockServiceWorker.js',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommendedTypeChecked,
  {
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
      globals: {
        ...globals.browser,
        ...globals.node,
      },
    },
  },
  {
    files: ['**/*.{ts,tsx,js,jsx}'],
    plugins: {
      react,
      'react-hooks': reactHooks,
      'jsx-a11y': jsxA11y,
    },
    settings: {
      // Pinned to React 19 (the version DECISIONS.md §12.4
      // commits to). Avoids `eslint-plugin-react`'s
      // detection-phase warning in packages that don't depend
      // on react directly.
      react: { version: '19' },
    },
    rules: {
      ...react.configs.flat.recommended.rules,
      ...react.configs.flat['jsx-runtime'].rules,
      ...reactHooks.configs.recommended.rules,
      ...jsxA11y.flatConfigs.recommended.rules,
    },
  },
  {
    // Config files are JS without a tsconfig project; opt them
    // out of the type-checked rule set.
    files: ['**/*.config.{js,cjs,mjs,ts}', 'eslint.config.js'],
    ...tseslint.configs.disableTypeChecked,
  },
  prettier,
);
