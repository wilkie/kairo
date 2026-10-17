// Unit tests for `<CreateObjectDialog />`.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CreateObjectDialog, type CreateObjectDialogProps } from '../src/CreateObjectDialog';

const ALICE = 'zMockActor00000000000000000000000000000000';

function renderDialog(overrides: Partial<CreateObjectDialogProps> = {}) {
  const props: CreateObjectDialogProps = {
    open: true,
    onClose: () => {},
    byActor: ALICE,
    onSubmit: () => {},
    ...overrides,
  };
  return { props, ...render(<CreateObjectDialog {...props} />) };
}

afterEach(() => {
  cleanup();
});

describe('<CreateObjectDialog />', () => {
  it('defaults the kind to software and submits it', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({ onSubmit });
    expect(screen.getByTestId('create-object-kind-input')).toHaveValue('software');
    await user.click(screen.getByTestId('create-object-submit'));
    expect(onSubmit).toHaveBeenCalledWith({ objectKind: 'software' });
  });

  it('accepts a free-text kind, trimmed', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({ onSubmit });
    const input = screen.getByTestId('create-object-kind-input');
    await user.clear(input);
    await user.type(input, '  dataset ');
    await user.click(screen.getByTestId('create-object-submit'));
    expect(onSubmit).toHaveBeenCalledWith({ objectKind: 'dataset' });
  });

  it('disables submit for an empty kind', async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.clear(screen.getByTestId('create-object-kind-input'));
    expect(screen.getByTestId('create-object-submit')).toBeDisabled();
  });

  it('offers the parent-supplied kinds alongside software', async () => {
    const user = userEvent.setup();
    renderDialog({ kindSuggestions: ['emulator', 'software'] });
    const input = screen.getByTestId('create-object-kind-input');
    await user.clear(input);
    await user.click(input);
    const options = (await screen.findAllByRole('option')).map((o) => o.textContent);
    expect(options).toEqual(['software', 'emulator']);
  });

  it('shows the pending state and the submit error', () => {
    renderDialog({
      submitting: true,
      submitError: { code: 'no_session', message: 'session expired' },
    });
    expect(screen.getByTestId('create-object-submit')).toBeDisabled();
    expect(screen.getByTestId('create-object-submit')).toHaveTextContent(/creating/i);
    expect(screen.getByTestId('create-object-error')).toHaveTextContent(/expired/);
  });

  it('resets to software when reopened', async () => {
    const user = userEvent.setup();
    const { props, rerender } = renderDialog();
    const input = screen.getByTestId('create-object-kind-input');
    await user.clear(input);
    await user.type(input, 'dataset');
    rerender(<CreateObjectDialog {...props} open={false} />);
    rerender(<CreateObjectDialog {...props} open />);
    expect(screen.getByTestId('create-object-kind-input')).toHaveValue('software');
  });
});
