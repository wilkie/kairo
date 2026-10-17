import type { Meta, StoryObj } from '@storybook/react';
import { CapabilityGrantDialog } from '../CapabilityGrantDialog';

const alice = 'zMockActor00000000000000000000000000000000';
const bob = 'zMockActorBob000000000000000000000000000000';
const carol = 'zMockActorCarol00000000000000000000000000000';
const alpha = 'zMockObject00000000000000000000000000000000';

const meta: Meta<typeof CapabilityGrantDialog> = {
  title: 'Auth/CapabilityGrantDialog',
  component: CapabilityGrantDialog,
  parameters: { layout: 'centered' },
};
export default meta;

type Story = StoryObj<typeof CapabilityGrantDialog>;

const base = {
  open: true,
  onClose: () => {},
  objectId: alpha,
  byActor: alice,
  granteeSuggestions: [bob, carol],
  existing: [{ grantee: bob, statementKinds: ['ObjectVersionTag' as const], delegable: true }],
  onSubmit: () => {},
};

export const Open: Story = { args: base };

export const Submitting: Story = { args: { ...base, submitting: true } };

export const RegrantDenied: Story = {
  args: {
    ...base,
    submitError: {
      code: 'not_authorized',
      message: 'actor zMockActorBob… may not grant ObjectBranch (not held with delegable=true)',
    },
  },
};
