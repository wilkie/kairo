import type { Meta, StoryObj } from '@storybook/react';
import { useState } from 'react';
import { TrustOpinionDialog, type TrustDecision } from '../TrustOpinionDialog';
import { Button } from '../Button';

const alice = 'zMockActor00000000000000000000000000000000';
const bob = 'zMockActorBob000000000000000000000000000000';

const meta: Meta<typeof TrustOpinionDialog> = {
  title: 'Auth/TrustOpinionDialog',
  component: TrustOpinionDialog,
  parameters: { layout: 'centered' },
};
export default meta;

type Story = StoryObj<typeof TrustOpinionDialog>;

export const Open: Story = {
  args: {
    open: true,
    onClose: () => {},
    trustedActor: bob,
    byActor: alice,
    onSubmit: () => {},
  },
};

export const Submitting: Story = {
  args: {
    open: true,
    onClose: () => {},
    trustedActor: bob,
    byActor: alice,
    submitting: true,
    onSubmit: () => {},
  },
};

export const WithError: Story = {
  args: {
    open: true,
    onClose: () => {},
    trustedActor: bob,
    byActor: alice,
    submitError: {
      code: 'network',
      message: 'Network error: could not reach the daemon.',
    },
    onSubmit: () => {},
  },
};

function InteractiveDemo() {
  const [open, setOpen] = useState(false);
  const [last, setLast] = useState<string | null>(null);

  return (
    <div>
      <Button variant="primary" onClick={() => setOpen(true)}>
        Express trust opinion
      </Button>
      {last !== null && <p data-testid="last-submission">Last submission: {last}</p>}
      <TrustOpinionDialog
        open={open}
        onClose={() => setOpen(false)}
        trustedActor={bob}
        byActor={alice}
        onSubmit={({
          decision,
          reason,
        }: {
          decision: TrustDecision;
          reason: string | undefined;
        }) => {
          setLast(`${decision}${reason !== undefined ? ` — ${reason}` : ''}`);
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
