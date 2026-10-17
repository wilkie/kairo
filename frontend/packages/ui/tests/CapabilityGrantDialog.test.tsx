// Unit tests for `<CapabilityGrantDialog />`.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {
  CapabilityGrantDialog,
  type CapabilityGrantDialogProps,
} from '../src/CapabilityGrantDialog';

const ALICE = 'zMockActor00000000000000000000000000000000';
const BOB = 'zMockActorBob000000000000000000000000000000';
const CAROL = 'zMockActorCarol00000000000000000000000000000';
const ALPHA = 'zMockObject00000000000000000000000000000000';

function renderDialog(overrides: Partial<CapabilityGrantDialogProps> = {}) {
  const props: CapabilityGrantDialogProps = {
    open: true,
    onClose: () => {},
    objectId: ALPHA,
    byActor: ALICE,
    granteeSuggestions: [BOB, CAROL],
    existing: [],
    onSubmit: () => {},
    ...overrides,
  };
  return { props, ...render(<CapabilityGrantDialog {...props} />) };
}

afterEach(() => {
  cleanup();
});

/** Paste rather than type: per-keystroke typing of a 44-char
 * actor id is slow enough to time out under a loaded test run. */
async function pasteGrantee(user: ReturnType<typeof userEvent.setup>, id: string) {
  await user.click(screen.getByTestId('capability-grantee-input'));
  await user.paste(id);
}

describe('<CapabilityGrantDialog />', () => {
  it('requires a grantee and at least one kind', async () => {
    const user = userEvent.setup();
    renderDialog();
    const submit = screen.getByTestId('capability-grant-submit');
    expect(submit).toBeDisabled();

    await pasteGrantee(user, CAROL);
    expect(submit).toBeDisabled();

    await user.click(screen.getByTestId('capability-kind-ObjectBranch'));
    expect(submit).toBeEnabled();
    expect(submit).toHaveTextContent(/^grant$/i);
  });

  it('submits a fresh grant with kinds in canonical order, not click order', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({ onSubmit });

    await pasteGrantee(user, CAROL);
    await user.click(screen.getByTestId('capability-kind-ObjectVersionTag'));
    await user.click(screen.getByTestId('capability-kind-ObjectRevision'));
    await user.click(screen.getByTestId('capability-delegable'));
    await user.click(screen.getByTestId('capability-grant-submit'));

    expect(onSubmit).toHaveBeenCalledWith({
      grantee: CAROL,
      statementKinds: ['ObjectRevision', 'ObjectVersionTag'],
      delegable: true,
    });
  });

  it('turns an expiry date into an RFC 3339 expires_at', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({ onSubmit });

    await pasteGrantee(user, CAROL);
    await user.click(screen.getByTestId('capability-kind-ObjectBranch'));
    await user.type(screen.getByTestId('capability-expires-input'), '2027-01-31');
    await user.click(screen.getByTestId('capability-grant-submit'));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ expiresAt: '2027-01-31T00:00:00Z' }),
    );
  });

  it('pre-fills an existing grant and switches to update mode', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({
      onSubmit,
      existing: [{ grantee: BOB, statementKinds: ['ObjectVersionTag'], delegable: true }],
    });

    await pasteGrantee(user, BOB);
    expect(screen.getByTestId('capability-kind-ObjectVersionTag')).toBeChecked();
    expect(screen.getByTestId('capability-delegable')).toBeChecked();
    expect(screen.getByTestId('capability-grant-mode-hint')).toHaveTextContent(/replaces/i);
    expect(screen.getByTestId('capability-grant-submit')).toHaveTextContent(/update/i);

    // Edits after the pre-fill stick: add a kind, then submit
    // the complete list.
    await user.click(screen.getByTestId('capability-kind-ObjectBranch'));
    expect(screen.getByTestId('capability-kind-ObjectBranch')).toBeChecked();
    await user.click(screen.getByTestId('capability-grant-submit'));
    expect(onSubmit).toHaveBeenCalledWith({
      grantee: BOB,
      statementKinds: ['ObjectBranch', 'ObjectVersionTag'],
      delegable: true,
    });
  });

  it('renders the submit error inline', () => {
    renderDialog({ submitError: { code: 'not_authorized', message: 'not held with delegable' } });
    expect(screen.getByTestId('capability-grant-error')).toHaveTextContent(/delegable/);
  });

  it('disables submit while pending', async () => {
    const user = userEvent.setup();
    renderDialog({ submitting: true });
    await pasteGrantee(user, CAROL);
    await user.click(screen.getByTestId('capability-kind-ObjectBranch'));
    expect(screen.getByTestId('capability-grant-submit')).toBeDisabled();
  });

  it('resets when reopened', async () => {
    const user = userEvent.setup();
    const { props, rerender } = renderDialog();
    await pasteGrantee(user, CAROL);
    await user.click(screen.getByTestId('capability-kind-ObjectBranch'));

    rerender(<CapabilityGrantDialog {...props} open={false} />);
    rerender(<CapabilityGrantDialog {...props} open />);
    expect(screen.getByTestId('capability-grantee-input')).toHaveValue('');
    expect(screen.getByTestId('capability-kind-ObjectBranch')).not.toBeChecked();
  });
});
