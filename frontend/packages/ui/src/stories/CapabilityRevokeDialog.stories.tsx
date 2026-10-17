import type { Meta, StoryObj } from '@storybook/react';
import { CapabilityRevokeDialog } from '../CapabilityRevokeDialog';

const meta: Meta<typeof CapabilityRevokeDialog> = {
  title: 'Auth/CapabilityRevokeDialog',
  component: CapabilityRevokeDialog,
  parameters: { layout: 'centered' },
};
export default meta;

type Story = StoryObj<typeof CapabilityRevokeDialog>;

const base = {
  open: true,
  onClose: () => {},
  grant: {
    statementId: 'zMockGrantStmt000000000000000000000000000000',
    grantee: 'zMockActorBob000000000000000000000000000000',
    statementKinds: ['ObjectBranch', 'ObjectVersionTag'],
  },
  onSubmit: () => {},
};

export const Open: Story = { args: base };

export const NotGrantor: Story = {
  args: {
    ...base,
    submitError: { code: 'not_authorized', message: 'only its grantor may revoke it' },
  },
};
