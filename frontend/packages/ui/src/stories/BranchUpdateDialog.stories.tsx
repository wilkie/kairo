import type { Meta, StoryObj } from '@storybook/react';
import { useState } from 'react';
import {
  BranchUpdateDialog,
  type BranchUpdateDialogRevision,
} from '../BranchUpdateDialog';
import { Button } from '../Button';

const alice = 'zMockActor00000000000000000000000000000000';
const alpha = 'zMockObject00000000000000000000000000000000';

const REVISIONS: BranchUpdateDialogRevision[] = [
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

const meta: Meta<typeof BranchUpdateDialog> = {
  title: 'Auth/BranchUpdateDialog',
  component: BranchUpdateDialog,
  parameters: { layout: 'centered' },
};
export default meta;

type Story = StoryObj<typeof BranchUpdateDialog>;

export const Open: Story = {
  args: {
    open: true,
    onClose: () => {},
    objectId: alpha,
    byActor: alice,
    existingNames: ['head', 'experimental'],
    revisions: REVISIONS,
    onSubmit: () => {},
  },
};

export const Submitting: Story = {
  args: {
    open: true,
    onClose: () => {},
    objectId: alpha,
    byActor: alice,
    existingNames: ['head'],
    revisions: REVISIONS,
    submitting: true,
    onSubmit: () => {},
  },
};

export const CapabilityDenied: Story = {
  args: {
    open: true,
    onClose: () => {},
    objectId: alpha,
    byActor: alice,
    existingNames: ['head'],
    revisions: REVISIONS,
    submitError: {
      code: 'not_authorized',
      message:
        'actor zMockActorBob… lacks an ObjectBranch capability for object zMockObject…',
    },
    onSubmit: () => {},
  },
};

export const NoRevisions: Story = {
  args: {
    open: true,
    onClose: () => {},
    objectId: alpha,
    byActor: alice,
    existingNames: [],
    revisions: [],
    onSubmit: () => {},
  },
};

function InteractiveDemo() {
  const [open, setOpen] = useState(false);
  const [last, setLast] = useState<string | null>(null);

  return (
    <div>
      <Button variant="primary" onClick={() => setOpen(true)}>
        Update branch
      </Button>
      {last !== null && <p data-testid="last-submission">Last submission: {last}</p>}
      <BranchUpdateDialog
        open={open}
        onClose={() => setOpen(false)}
        objectId={alpha}
        byActor={alice}
        existingNames={['head', 'experimental']}
        revisions={REVISIONS}
        onSubmit={({ name, revisionStatementId }) => {
          setLast(`${name} → ${revisionStatementId.slice(0, 20)}…`);
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
