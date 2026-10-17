// Unit tests for `<VersionTagDialog />`. Mirrors the
// BranchUpdateDialog test shape — render fresh, drive
// interactions via testing-library, assert dispatch + render.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {
  VersionTagDialog,
  isStrictSemver,
  type VersionTagDialogProps,
  type VersionTagDialogRevision,
} from '../src/VersionTagDialog';

const ALICE = 'zMockActor00000000000000000000000000000000';
const ALPHA = 'zMockObject00000000000000000000000000000000';

const REV_NEW: VersionTagDialogRevision = {
  statementId: 'zMockRevStmt3',
  revisionId: 'git:sha1:0003',
  createdAt: '2026-01-03T00:00:00Z',
};
const REV_OLD: VersionTagDialogRevision = {
  statementId: 'zMockRevStmt1',
  revisionId: 'git:sha1:0001',
  createdAt: '2026-01-01T00:00:00Z',
};

function renderDialog(overrides: Partial<VersionTagDialogProps> = {}) {
  const props: VersionTagDialogProps = {
    open: true,
    onClose: () => {},
    objectId: ALPHA,
    byActor: ALICE,
    existingVersions: [],
    revisions: [REV_NEW, REV_OLD],
    onSubmit: () => {},
    ...overrides,
  };
  return { props, ...render(<VersionTagDialog {...props} />) };
}

afterEach(() => {
  cleanup();
});

describe('isStrictSemver', () => {
  it.each(['1.0.0', '0.1.0', '2.0.0-rc.1', '1.2.3+build.5', '1.0.0-alpha.beta+exp.sha.5114f85'])(
    'accepts %s',
    (value) => {
      expect(isStrictSemver(value)).toBe(true);
    },
  );

  it.each(['v1.0.0', '1.0', '01.0.0', '1.0.0-01', '1.0.0 ', 'latest', ''])(
    'rejects %j',
    (value) => {
      expect(isStrictSemver(value)).toBe(false);
    },
  );
});

describe('<VersionTagDialog />', () => {
  it('renders the form when open', () => {
    renderDialog();
    expect(screen.getByTestId('version-tag-form')).toBeInTheDocument();
    expect(screen.getByTestId('version-tag-input')).toBeInTheDocument();
    expect(screen.getByTestId('version-tag-submit')).toBeInTheDocument();
  });

  it('does not render when closed', () => {
    renderDialog({ open: false });
    expect(screen.queryByTestId('version-tag-form')).not.toBeInTheDocument();
  });

  it('disables submit until a strict semver version is supplied', async () => {
    const user = userEvent.setup();
    renderDialog();
    const submit = screen.getByTestId('version-tag-submit');
    expect(submit).toBeDisabled();

    await user.type(screen.getByTestId('version-tag-input'), '1.0');
    expect(submit).toBeDisabled();
    expect(screen.getByText(/not a strict semver/i)).toBeInTheDocument();

    await user.type(screen.getByTestId('version-tag-input'), '.0');
    expect(submit).toBeEnabled();
  });

  it('suggests dropping a leading v', async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.type(screen.getByTestId('version-tag-input'), 'v2.0.0');
    expect(screen.getByTestId('version-tag-submit')).toBeDisabled();
    expect(screen.getByText(/drop the leading "v": 2\.0\.0/i)).toBeInTheDocument();
  });

  it('submits with the newest revision pre-selected', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({ onSubmit });

    await user.type(screen.getByTestId('version-tag-input'), '2.0.0');
    await user.click(screen.getByTestId('version-tag-submit'));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith({
      version: '2.0.0',
      revisionStatementId: REV_NEW.statementId,
    });
  });

  it('shows the create hint for a version this actor has not tagged', async () => {
    const user = userEvent.setup();
    renderDialog({ existingVersions: ['1.0.0'] });
    await user.type(screen.getByTestId('version-tag-input'), '1.2.0');
    expect(screen.getByTestId('version-tag-mode-hint')).toHaveTextContent(
      /creates a new 1\.2\.0 tag/i,
    );
    expect(screen.getByTestId('version-tag-submit')).toHaveTextContent(/create/i);
  });

  it('shows the retarget hint for a version this actor already tagged', async () => {
    const user = userEvent.setup();
    renderDialog({ existingVersions: ['1.0.0'] });
    await user.type(screen.getByTestId('version-tag-input'), '1.0.0');
    expect(screen.getByTestId('version-tag-mode-hint')).toHaveTextContent(
      /replaces your prior 1\.0\.0 tag/i,
    );
    expect(screen.getByTestId('version-tag-submit')).toHaveTextContent(/retarget/i);
  });

  it('honors a user-picked older revision', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderDialog({ onSubmit });

    await user.type(screen.getByTestId('version-tag-input'), '0.9.0');
    await user.click(screen.getByRole('combobox', { name: /revision/i }));
    const listbox = await screen.findByRole('listbox');
    await user.click(
      within(listbox).getByTestId(`version-tag-revision-option-${REV_OLD.statementId}`),
    );
    await user.click(screen.getByTestId('version-tag-submit'));

    expect(onSubmit).toHaveBeenCalledWith({
      version: '0.9.0',
      revisionStatementId: REV_OLD.statementId,
    });
  });

  it('disables submit while pending and shows the spinner label', async () => {
    const user = userEvent.setup();
    renderDialog({ submitting: true });
    await user.type(screen.getByTestId('version-tag-input'), '1.0.0');
    const submit = screen.getByTestId('version-tag-submit');
    expect(submit).toBeDisabled();
    expect(submit).toHaveTextContent(/submitting/i);
  });

  it('renders the submit error inline', () => {
    renderDialog({
      submitError: {
        code: 'not_authorized',
        message: 'actor lacks an ObjectVersionTag capability',
      },
    });
    expect(screen.getByTestId('version-tag-error')).toHaveTextContent(
      /lacks an ObjectVersionTag capability/i,
    );
  });

  it('disables submit when there are no revisions', async () => {
    const user = userEvent.setup();
    renderDialog({ revisions: [] });
    await user.type(screen.getByTestId('version-tag-input'), '1.0.0');
    expect(screen.getByTestId('version-tag-submit')).toBeDisabled();
    expect(screen.getByText(/no revisions available/i)).toBeInTheDocument();
  });

  it('fires onClose when the cancel button is clicked', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderDialog({ onClose });
    await user.click(screen.getByRole('button', { name: /cancel/i }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('resets the form fields when the dialog reopens', async () => {
    const user = userEvent.setup();
    const { props, rerender } = renderDialog();
    await user.type(screen.getByTestId('version-tag-input'), '1.0.0');
    expect(screen.getByTestId('version-tag-input')).toHaveValue('1.0.0');

    rerender(<VersionTagDialog {...props} open={false} />);
    rerender(<VersionTagDialog {...props} open />);
    expect(screen.getByTestId('version-tag-input')).toHaveValue('');
  });
});
