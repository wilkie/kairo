import type { Meta, StoryObj } from '@storybook/react';
import { useState } from 'react';
import { VersionTagDialog, type VersionTagDialogRevision } from '../VersionTagDialog';
import { Button } from '../Button';

const alice = 'zMockActor00000000000000000000000000000000';
const alpha = 'zMockObject00000000000000000000000000000000';

const REVISIONS: VersionTagDialogRevision[] = [
  {
    statementId: 'zMockRevisionStmtAlpha3000000000000000000',
    revisionId: 'git:sha256:0000000000000000000000000000000000000003',
    createdAt: '2026-01-03T00:00:00Z',
  },
  {
    statementId: 'zMockRevisionStmtAlpha2000000000000000000',
    revisionId: 'git:sha256:0000000000000000000000000000000000000002',
    createdAt: '2026-01-02T00:00:00Z',
  },
  {
    statementId: 'zMockRevisionStmt00000000000000000000000000',
    revisionId: 'git:sha256:0000000000000000000000000000000000000001',
    createdAt: '2026-01-01T00:00:00Z',
  },
];

const meta: Meta<typeof VersionTagDialog> = {
  title: 'Auth/VersionTagDialog',
  component: VersionTagDialog,
  parameters: { layout: 'centered' },
};
export default meta;

type Story = StoryObj<typeof VersionTagDialog>;

const base = {
  open: true,
  onClose: () => {},
  objectId: alpha,
  byActor: alice,
  existingVersions: ['1.0.0', '1.1.0'],
  revisions: REVISIONS,
  onSubmit: () => {},
};

export const Open: Story = { args: base };

export const Submitting: Story = { args: { ...base, submitting: true } };

export const CapabilityDenied: Story = {
  args: {
    ...base,
    submitError: {
      code: 'not_authorized',
      message: 'actor zMockActorBob… lacks an ObjectVersionTag capability for object zMockObject…',
    },
  },
};

export const NoRevisions: Story = { args: { ...base, existingVersions: [], revisions: [] } };

function InteractiveDemo() {
  const [open, setOpen] = useState(false);
  const [last, setLast] = useState<string | null>(null);

  return (
    <div>
      <Button variant="primary" onClick={() => setOpen(true)}>
        Tag version
      </Button>
      {last !== null && <p data-testid="last-submission">Last submission: {last}</p>}
      <VersionTagDialog
        {...base}
        open={open}
        onClose={() => setOpen(false)}
        onSubmit={({ version, revisionStatementId }) => {
          setLast(`${version} → ${revisionStatementId.slice(0, 20)}…`);
          setOpen(false);
        }}
      />
    </div>
  );
}

export const Interactive: Story = {
  name: 'Interactive (click through)',
  render: () => <InteractiveDemo />,
};
