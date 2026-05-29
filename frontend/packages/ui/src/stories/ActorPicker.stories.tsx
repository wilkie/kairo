import type { Meta, StoryObj } from '@storybook/react';
import { useState } from 'react';
import { ActorPicker, type ActorPickerSession } from '../ActorPicker';

const alice = { actor_id: 'zMockActor00000000000000000000000000000000' };
const bob = { actor_id: 'zMockActorBob000000000000000000000000000000' };

const meta: Meta<typeof ActorPicker> = {
  title: 'Auth/ActorPicker',
  component: ActorPicker,
  parameters: { layout: 'padded' },
};
export default meta;

type Story = StoryObj<typeof ActorPicker>;

export const Anonymous: Story = {
  args: {
    session: null,
    actors: [alice, bob],
    onSignIn: () => {},
    onSignOut: () => {},
  },
};

export const SignedIn: Story = {
  args: {
    session: { actor_id: alice.actor_id },
    actors: [alice, bob],
    onSignIn: () => {},
    onSignOut: () => {},
  },
};

export const EmptyKeystore: Story = {
  name: 'Anonymous • empty keystore',
  args: {
    session: null,
    actors: [],
    onSignIn: () => {},
    onSignOut: () => {},
  },
};

export const ActorsLoading: Story = {
  name: 'Anonymous • actors loading',
  args: {
    session: null,
    actors: [],
    actorsLoading: true,
    onSignIn: () => {},
    onSignOut: () => {},
  },
};

export const SignInPending: Story = {
  name: 'Confirmation • pending',
  args: {
    session: null,
    actors: [alice, bob],
    signInPending: true,
    onSignIn: () => {},
    onSignOut: () => {},
  },
};

export const RevokedKeyError: Story = {
  name: 'Confirmation • revoked-key error',
  args: {
    session: null,
    actors: [alice, bob],
    signInError: {
      code: 'key_revoked',
      message: 'active signing key for actor is revoked',
    },
    onSignIn: () => {},
    onSignOut: () => {},
  },
};

// Live demo: drives the full state machine so reviewers can
// click through anonymous → popover → confirm → signed-in →
// sign-out without writing any callback wiring.
function LiveDemo() {
  const [session, setSession] = useState<ActorPickerSession | null>(null);
  const [pending, setPending] = useState(false);

  return (
    <ActorPicker
      session={session}
      actors={[alice, bob]}
      signInPending={pending}
      onSignIn={(actorId) => {
        setPending(true);
        setTimeout(() => {
          setSession({ actor_id: actorId });
          setPending(false);
        }, 400);
      }}
      onSignOut={() => setSession(null)}
    />
  );
}

export const Interactive: Story = {
  name: 'Interactive (click through)',
  render: () => <LiveDemo />,
};
