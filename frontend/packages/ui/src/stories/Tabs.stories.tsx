import type { Meta, StoryObj } from '@storybook/react';
import { Tabs } from '../Tabs';

const meta: Meta<typeof Tabs> = {
  title: 'Primitives/Tabs',
  component: Tabs,
};
export default meta;

type Story = StoryObj<typeof Tabs>;

export const Default: Story = {
  args: {
    tabs: [
      { id: 'genesis', label: 'Genesis', content: <p>Genesis envelope renders here.</p> },
      { id: 'branches', label: 'Branches', content: <p>Branch tips table.</p> },
      { id: 'tags', label: 'Tags', content: <p>Version tags table.</p> },
    ],
  },
};
