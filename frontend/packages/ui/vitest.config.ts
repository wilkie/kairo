// Vitest config for `@kairo/ui` component tests.
//
// happy-dom gives us a DOM API in Node without the weight of
// jsdom; @testing-library/react drives the test interaction
// surface. Setup file installs jest-dom matchers so
// `toBeInTheDocument` etc. land on `expect`.

import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'happy-dom',
    globals: false,
    setupFiles: ['./tests/setup.ts'],
    include: ['tests/**/*.test.ts', 'tests/**/*.test.tsx'],
  },
});
