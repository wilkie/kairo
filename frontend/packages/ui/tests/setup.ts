// jest-dom's `/vitest` entry calls `expect.extend` on whichever
// `vitest` it resolves — in this monorepo that is the 2.x copy
// the other packages depend on, not this package's 4.x — so the
// matchers never reached our `expect`. Register them on our own
// copy explicitly; the type-only import keeps the `Assertion`
// augmentation for `toBeInTheDocument` and friends.
import type {} from '@testing-library/jest-dom/vitest';
import * as matchers from '@testing-library/jest-dom/matchers';
import { expect } from 'vitest';

expect.extend(matchers);
