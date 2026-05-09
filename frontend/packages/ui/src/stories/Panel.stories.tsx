import type { Meta, StoryObj } from '@storybook/react';
import { Panel } from '../Panel';
import { Button } from '../Button';

const meta: Meta<typeof Panel> = {
  title: 'Primitives/Panel',
  component: Panel,
};
export default meta;

type Story = StoryObj<typeof Panel>;

export const Bare: Story = {
  args: {
    children: <p>Panels group related content. This one has no header.</p>,
  },
};

export const WithHeader: Story = {
  args: {
    title: 'Branches',
    description: 'One row per (actor, name) chain leaf.',
    children: <p>Branch tip table renders here.</p>,
  },
};

export const WithActions: Story = {
  args: {
    title: 'Daemon',
    description: 'Live read of /api/v1/version.',
    actions: <Button variant="primary">Refresh</Button>,
    children: <p>Version metadata renders here.</p>,
  },
};
