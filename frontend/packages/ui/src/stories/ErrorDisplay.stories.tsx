import type { Meta, StoryObj } from '@storybook/react';
import { ErrorDisplay } from '../ErrorDisplay';
import { Button } from '../Button';

const meta: Meta<typeof ErrorDisplay> = {
  title: 'Primitives/ErrorDisplay',
  component: ErrorDisplay,
};
export default meta;

type Story = StoryObj<typeof ErrorDisplay>;

export const NetworkFailure: Story = {
  args: {
    title: 'Could not reach the daemon',
    message: 'The version endpoint is unreachable.',
    detail: 'network: ECONNREFUSED 127.0.0.1:7878',
  },
};

export const DaemonError: Story = {
  args: {
    title: 'Object not found',
    message: 'The daemon reported the object id was not in the local store.',
    detail: 'daemon: not_found (HTTP 404)',
    actions: <Button>Retry</Button>,
  },
};

export const DecodeError: Story = {
  args: {
    title: 'Could not decode response',
    message: 'The daemon returned an unexpected JSON shape.',
    detail: 'decode: envelope missing required `ok` discriminator',
  },
};
