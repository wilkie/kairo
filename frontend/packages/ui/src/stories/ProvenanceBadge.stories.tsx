import type { Meta, StoryObj } from '@storybook/react';
import { ProvenanceBadge } from '../ProvenanceBadge';

const meta: Meta<typeof ProvenanceBadge> = {
  title: 'Primitives/ProvenanceBadge',
  component: ProvenanceBadge,
};
export default meta;

type Story = StoryObj<typeof ProvenanceBadge>;

const archive = {
  peerUrl: 'https://archive.example.org',
  peerActor: 'zMockActorCarol00000000000000000000000000',
  fetchedAt: '2026-10-01T12:00:00Z',
};

export const OnePeer: Story = { args: { entries: [archive] } };

export const TwoPeers: Story = {
  args: {
    entries: [
      archive,
      { ...archive, peerUrl: 'https://mirror.example.net', fetchedAt: '2026-10-02T08:30:00Z' },
    ],
  },
};

export const Local: Story = { name: 'Local (renders nothing)', args: { entries: [] } };
