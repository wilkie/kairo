// Unit tests for `<TrustOpinionDialog />`. The component is
// dumb — callbacks and state come in as props — so each test
// renders a fresh tree and asserts the rendered shape /
// dispatch.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TrustOpinionDialog } from '../src/TrustOpinionDialog';

const ALICE = 'zMockActor00000000000000000000000000000000';
const BOB = 'zMockActorBob000000000000000000000000000000';

afterEach(() => {
  cleanup();
});

describe('<TrustOpinionDialog />', () => {
  it('renders the form when open', () => {
    render(
      <TrustOpinionDialog
        open
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={() => {}}
      />,
    );

    expect(screen.getByTestId('trust-opinion-form')).toBeInTheDocument();
    expect(screen.getByTestId('trust-opinion-submit')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument();
    // Decision defaults to trusted.
    expect(screen.getByRole('radio', { name: 'Trusted' })).toBeChecked();
  });

  it('does not render when closed', () => {
    render(
      <TrustOpinionDialog
        open={false}
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={() => {}}
      />,
    );
    expect(screen.queryByTestId('trust-opinion-form')).not.toBeInTheDocument();
  });

  it('submits with the trusted decision by default', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <TrustOpinionDialog
        open
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={onSubmit}
      />,
    );

    await user.click(screen.getByTestId('trust-opinion-submit'));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith({
      decision: 'trusted',
      reason: undefined,
    });
  });

  it('honors the user-selected untrusted decision', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <TrustOpinionDialog
        open
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={onSubmit}
      />,
    );

    await user.click(screen.getByRole('radio', { name: 'Untrusted' }));
    await user.click(screen.getByTestId('trust-opinion-submit'));

    expect(onSubmit).toHaveBeenCalledWith({
      decision: 'untrusted',
      reason: undefined,
    });
  });

  it('passes a non-empty reason through trimmed', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <TrustOpinionDialog
        open
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={onSubmit}
      />,
    );

    await user.type(screen.getByTestId('trust-reason-input'), '   ci-smoke  ');
    await user.click(screen.getByTestId('trust-opinion-submit'));

    expect(onSubmit).toHaveBeenCalledWith({
      decision: 'trusted',
      reason: 'ci-smoke',
    });
  });

  it('treats a blank reason as undefined', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <TrustOpinionDialog
        open
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={onSubmit}
      />,
    );

    await user.type(screen.getByTestId('trust-reason-input'), '   ');
    await user.click(screen.getByTestId('trust-opinion-submit'));

    expect(onSubmit).toHaveBeenCalledWith({
      decision: 'trusted',
      reason: undefined,
    });
  });

  it('disables submit while pending and shows the spinner label', () => {
    render(
      <TrustOpinionDialog
        open
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        submitting
        onSubmit={() => {}}
      />,
    );

    const submit = screen.getByTestId('trust-opinion-submit');
    expect(submit).toBeDisabled();
    expect(submit).toHaveTextContent(/submitting/i);
    expect(screen.getByRole('button', { name: /cancel/i })).toBeDisabled();
  });

  it('renders the submit error inline', () => {
    render(
      <TrustOpinionDialog
        open
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        submitError={{ code: 'network', message: 'could not reach the daemon' }}
        onSubmit={() => {}}
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent(/could not reach the daemon/i);
  });

  it('fires onClose when the cancel button is clicked', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <TrustOpinionDialog
        open
        onClose={onClose}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /cancel/i }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('resets the form fields when the dialog reopens', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const { rerender } = render(
      <TrustOpinionDialog
        open
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={onSubmit}
      />,
    );

    await user.click(screen.getByRole('radio', { name: 'Untrusted' }));
    await user.type(screen.getByTestId('trust-reason-input'), 'first');

    // Close.
    rerender(
      <TrustOpinionDialog
        open={false}
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={onSubmit}
      />,
    );
    // Reopen — fields should reset.
    rerender(
      <TrustOpinionDialog
        open
        onClose={() => {}}
        trustedActor={BOB}
        byActor={ALICE}
        onSubmit={onSubmit}
      />,
    );

    expect(screen.getByRole('radio', { name: 'Trusted' })).toBeChecked();
    expect(screen.getByTestId('trust-reason-input')).toHaveValue('');
  });
});
