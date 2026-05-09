import type { Meta, StoryObj } from '@storybook/react';
import { StatusBadge } from '../StatusBadge';

const meta: Meta<typeof StatusBadge> = {
  title: 'Primitives/StatusBadge',
  component: StatusBadge,
};
export default meta;

type Story = StoryObj<typeof StatusBadge>;

export const Tones: Story = {
  render: () => (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.5rem' }}>
      <StatusBadge tone="neutral">Neutral</StatusBadge>
      <StatusBadge tone="info">Info</StatusBadge>
      <StatusBadge tone="warn">Warning</StatusBadge>
      <StatusBadge tone="error">Error</StatusBadge>
      <StatusBadge tone="success">Success</StatusBadge>
    </div>
  ),
};

export const ValidationStatuses: Story = {
  name: 'Validation statuses',
  render: () => (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.5rem' }}>
      <StatusBadge tone="success">Valid</StatusBadge>
      <StatusBadge tone="error">Invalid</StatusBadge>
      <StatusBadge tone="warn">Conflicted</StatusBadge>
      <StatusBadge tone="info">Indeterminate</StatusBadge>
      <StatusBadge tone="neutral">Unverified</StatusBadge>
    </div>
  ),
};
