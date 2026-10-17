// Unit tests for `<BranchUpdateDialog />`. Mirrors the
// TrustOpinionDialog test shape — render fresh, drive
// interactions via testing-library, assert dispatch + render.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { BranchUpdateDialog, type BranchUpdateDialogRevision } from '../src/BranchUpdateDialog';

const ALICE = 'zMockActor00000000000000000000000000000000';
const ALPHA = 'zMockObject00000000000000000000000000000000';

const REV_NEW: BranchUpdateDialogRevision = {
  statementId: 'zMockRevStmt3',
  revisionId: 'git:sha256:0003',
  createdAt: '2026-01-03T00:00:00Z',
};
const REV_OLD: BranchUpdateDialogRevision = {
  statementId: 'zMockRevStmt1',
  revisionId: 'git:sha256:0001',
  createdAt: '2026-01-01T00:00:00Z',
};

const REVISIONS = [REV_NEW, REV_OLD];

afterEach(() => {
  cleanup();
});

describe('<BranchUpdateDialog />', () => {
  it('renders the form when open', () => {
    render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        onSubmit={() => {}}
      />,
    );
    expect(screen.getByTestId('branch-update-form')).toBeInTheDocument();
    expect(screen.getByTestId('branch-name-input')).toBeInTheDocument();
    expect(screen.getByTestId('branch-update-submit')).toBeInTheDocument();
  });

  it('does not render when closed', () => {
    render(
      <BranchUpdateDialog
        open={false}
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        onSubmit={() => {}}
      />,
    );
    expect(screen.queryByTestId('branch-update-form')).not.toBeInTheDocument();
  });

  it('disables submit until a name is supplied', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        onSubmit={onSubmit}
      />,
    );

    expect(screen.getByTestId('branch-update-submit')).toBeDisabled();
    await user.type(screen.getByTestId('branch-name-input'), 'head');
    expect(screen.getByTestId('branch-update-submit')).toBeEnabled();
  });

  it('submits with the newest revision pre-selected', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        onSubmit={onSubmit}
      />,
    );

    await user.type(screen.getByTestId('branch-name-input'), 'feature');
    await user.click(screen.getByTestId('branch-update-submit'));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith({
      name: 'feature',
      revisionStatementId: REV_NEW.statementId,
    });
  });

  it('shows the create hint when the name does not match an existing tip', async () => {
    const user = userEvent.setup();
    render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={['head']}
        revisions={REVISIONS}
        onSubmit={() => {}}
      />,
    );

    await user.type(screen.getByTestId('branch-name-input'), 'release-1.x');
    expect(screen.getByTestId('branch-mode-hint')).toHaveTextContent(/creates a new branch/i);
    expect(screen.getByTestId('branch-update-submit')).toHaveTextContent(/create/i);
  });

  it('shows the update hint when the name matches an existing tip', async () => {
    const user = userEvent.setup();
    render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={['head']}
        revisions={REVISIONS}
        onSubmit={() => {}}
      />,
    );

    await user.type(screen.getByTestId('branch-name-input'), 'head');
    expect(screen.getByTestId('branch-mode-hint')).toHaveTextContent(/replaces your prior tip/i);
    expect(screen.getByTestId('branch-update-submit')).toHaveTextContent(/update/i);
  });

  it('disables submit while pending and shows the spinner label', async () => {
    const user = userEvent.setup();
    render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        submitting
        onSubmit={() => {}}
      />,
    );

    // Supply a name so the disabled would otherwise be due to
    // empty-name; the submitting flag should still keep it
    // disabled.
    await user.type(screen.getByTestId('branch-name-input'), 'head');
    const submit = screen.getByTestId('branch-update-submit');
    expect(submit).toBeDisabled();
    expect(submit).toHaveTextContent(/submitting/i);
  });

  it('renders the submit error inline', () => {
    render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        submitError={{
          code: 'not_authorized',
          message: 'actor lacks an ObjectBranch capability',
        }}
        onSubmit={() => {}}
      />,
    );

    const alert = screen.getByTestId('branch-update-error');
    expect(alert).toHaveTextContent(/lacks an ObjectBranch capability/i);
  });

  it('disables submit when there are no revisions', () => {
    render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={[]}
        onSubmit={() => {}}
      />,
    );

    expect(screen.getByTestId('branch-update-submit')).toBeDisabled();
    expect(screen.getByText(/no revisions available/i)).toBeInTheDocument();
  });

  it('honors a user-picked older revision', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        onSubmit={onSubmit}
      />,
    );

    await user.type(screen.getByTestId('branch-name-input'), 'rollback');
    // Open the select and pick the older revision.
    await user.click(screen.getByRole('combobox', { name: /revision/i }));
    const listbox = await screen.findByRole('listbox');
    await user.click(within(listbox).getByTestId(`branch-revision-option-${REV_OLD.statementId}`));
    await user.click(screen.getByTestId('branch-update-submit'));

    expect(onSubmit).toHaveBeenCalledWith({
      name: 'rollback',
      revisionStatementId: REV_OLD.statementId,
    });
  });

  it('fires onClose when the cancel button is clicked', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <BranchUpdateDialog
        open
        onClose={onClose}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        onSubmit={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /cancel/i }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('resets the form fields when the dialog reopens', async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        onSubmit={() => {}}
      />,
    );

    await user.type(screen.getByTestId('branch-name-input'), 'feature');
    expect(screen.getByTestId('branch-name-input')).toHaveValue('feature');

    rerender(
      <BranchUpdateDialog
        open={false}
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        onSubmit={() => {}}
      />,
    );
    rerender(
      <BranchUpdateDialog
        open
        onClose={() => {}}
        objectId={ALPHA}
        byActor={ALICE}
        existingNames={[]}
        revisions={REVISIONS}
        onSubmit={() => {}}
      />,
    );

    expect(screen.getByTestId('branch-name-input')).toHaveValue('');
  });
});
