import type { Meta, StoryObj } from '@storybook/react';
import { EmptyState } from '../EmptyState';
import { Button } from '../Button';

const meta: Meta<typeof EmptyState> = {
  title: 'Primitives/EmptyState',
  component: EmptyState,
};
export default meta;

type Story = StoryObj<typeof EmptyState>;

export const Default: Story = {
  args: {
    title: 'No branches yet',
    description: 'This object has no signed branch tips. Sign one with `kairo branch set`.',
  },
};

export const WithAction: Story = {
  args: {
    title: 'Object listing is a slice-8 follow-up',
    description: 'Once a /api/v1/objects listing endpoint exists on the daemon, the inspector will surface it here.',
    actions: <Button variant="primary">Refresh</Button>,
  },
};
