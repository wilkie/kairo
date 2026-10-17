import type { Meta, StoryObj } from '@storybook/react';
import { CreateObjectDialog } from '../CreateObjectDialog';

const meta: Meta<typeof CreateObjectDialog> = {
  title: 'Auth/CreateObjectDialog',
  component: CreateObjectDialog,
  parameters: { layout: 'centered' },
};
export default meta;

type Story = StoryObj<typeof CreateObjectDialog>;

const base = {
  open: true,
  onClose: () => {},
  byActor: 'zMockActor00000000000000000000000000000000',
  kindSuggestions: ['emulator', 'dataset'],
  onSubmit: () => {},
};

export const Open: Story = { args: base };
export const Submitting: Story = { args: { ...base, submitting: true } };
export const SessionExpired: Story = {
  args: {
    ...base,
    submitError: { code: 'no_session', message: 'no live session for that cookie' },
  },
};
