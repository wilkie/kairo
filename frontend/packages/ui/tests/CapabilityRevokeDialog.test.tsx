// Unit tests for `<CapabilityRevokeDialog />`.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {
  CapabilityRevokeDialog,
  type CapabilityRevokeDialogProps,
} from '../src/CapabilityRevokeDialog';

const GRANT = {
  statementId: 'zMockGrantStmt000000000000000000000000000000',
  grantee: 'zMockActorBob000000000000000000000000000000',
  statementKinds: ['ObjectVersionTag'],
};

function renderDialog(overrides: Partial<CapabilityRevokeDialogProps> = {}) {
  const props: CapabilityRevokeDialogProps = {
    open: true,
    onClose: () => {},
    grant: GRANT,
    onSubmit: () => {},
    ...overrides,
  };
  return { props, ...render(<CapabilityRevokeDialog {...props} />) };
}

afterEach(() => {
  cleanup();
});

describe('<CapabilityRevokeDialog />', () => {
  it('defaults to a forward-only revocation with an optional reason', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({ onSubmit });

    expect(screen.getByTestId('capability-revoke-retroactive')).not.toBeChecked();
    expect(screen.queryByTestId('capability-revoke-retroactive-warning')).not.toBeInTheDocument();
    await user.click(screen.getByTestId('capability-revoke-submit'));
    expect(onSubmit).toHaveBeenCalledWith({ retroactive: false });
  });

  it('passes a trimmed reason through', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({ onSubmit });
    await user.type(screen.getByTestId('capability-revoke-reason'), '  rotating  ');
    await user.click(screen.getByTestId('capability-revoke-submit'));
    expect(onSubmit).toHaveBeenCalledWith({ retroactive: false, reason: 'rotating' });
  });

  it('warns and requires a reason when retroactive', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({ onSubmit });

    await user.click(screen.getByTestId('capability-revoke-retroactive'));
    expect(screen.getByTestId('capability-revoke-retroactive-warning')).toBeInTheDocument();
    const submit = screen.getByTestId('capability-revoke-submit');
    expect(submit).toBeDisabled();
    expect(submit).toHaveTextContent(/retroactively/i);

    await user.type(screen.getByTestId('capability-revoke-reason'), 'grantee key compromised');
    await user.click(submit);
    expect(onSubmit).toHaveBeenCalledWith({
      retroactive: true,
      reason: 'grantee key compromised',
    });
  });

  it('renders nothing without a grant', () => {
    renderDialog({ grant: null });
    expect(screen.queryByTestId('capability-revoke-form')).not.toBeInTheDocument();
  });

  it('renders the submit error inline', () => {
    renderDialog({
      submitError: { code: 'not_authorized', message: 'only its grantor may revoke' },
    });
    expect(screen.getByTestId('capability-revoke-error')).toHaveTextContent(/grantor/);
  });

  it('resets the retroactive flag when reopened', async () => {
    const user = userEvent.setup();
    const { props, rerender } = renderDialog();
    await user.click(screen.getByTestId('capability-revoke-retroactive'));
    rerender(<CapabilityRevokeDialog {...props} open={false} />);
    rerender(<CapabilityRevokeDialog {...props} open />);
    expect(screen.getByTestId('capability-revoke-retroactive')).not.toBeChecked();
  });
});
