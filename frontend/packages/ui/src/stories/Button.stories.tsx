import type { Meta, StoryObj } from '@storybook/react';
import { Button } from '../Button';

const meta: Meta<typeof Button> = {
  title: 'Primitives/Button',
  component: Button,
  args: {
    children: 'Sign statement',
  },
  argTypes: {
    variant: {
      control: { type: 'inline-radio' },
      options: ['default', 'primary', 'ghost'],
    },
  },
};
export default meta;

type Story = StoryObj<typeof Button>;

export const Default: Story = {
  args: { variant: 'default' },
};

export const Primary: Story = {
  args: { variant: 'primary' },
};

export const Ghost: Story = {
  args: { variant: 'ghost' },
};

export const Disabled: Story = {
  args: { disabled: true },
};
