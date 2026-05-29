// Unit tests for `<ActorPicker />`. The component is dumb —
// callbacks and current state come in as props — so each test
// renders a fresh tree and asserts the rendered shape /
// dispatch.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ActorPicker, truncateActorId } from '../src/ActorPicker';

const ALICE = { actor_id: 'zMockActor00000000000000000000000000000000' };
const BOB = { actor_id: 'zMockActorBob000000000000000000000000000000' };

afterEach(() => {
  cleanup();
});

describe('truncateActorId', () => {
  it('shortens long ids with a middle ellipsis', () => {
    const truncated = truncateActorId(ALICE.actor_id);
    expect(truncated.length).toBeLessThan(ALICE.actor_id.length);
    expect(truncated).toContain('…');
    expect(truncated.startsWith(ALICE.actor_id.slice(0, 4))).toBe(true);
    expect(truncated.endsWith(ALICE.actor_id.slice(-4))).toBe(true);
  });

  it('returns short ids verbatim', () => {
    expect(truncateActorId('short')).toBe('short');
  });
});

describe('<ActorPicker /> — anonymous', () => {
  it('renders a sign-in button', () => {
    render(
      <ActorPicker
        session={null}
        actors={[ALICE, BOB]}
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );
    expect(screen.getByRole('button', { name: /sign in/i })).toBeInTheDocument();
    // The signed-in label should not be present.
    expect(screen.queryByTestId('actor-picker-signed-in')).not.toBeInTheDocument();
  });

  it('opens a popover listing keystore actors', async () => {
    const user = userEvent.setup();
    render(
      <ActorPicker
        session={null}
        actors={[ALICE, BOB]}
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign in/i }));

    const items = await screen.findAllByTestId('actor-picker-actor');
    expect(items).toHaveLength(2);
    // Truncated label for each actor.
    expect(items[0]).toHaveTextContent(truncateActorId(ALICE.actor_id));
  });

  it('shows a hint when the keystore is empty', async () => {
    const user = userEvent.setup();
    render(
      <ActorPicker
        session={null}
        actors={[]}
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign in/i }));

    expect(await screen.findByText(/no local actors/i)).toBeInTheDocument();
  });

  it('shows a loading spinner while actors are loading', async () => {
    const user = userEvent.setup();
    render(
      <ActorPicker
        session={null}
        actors={[]}
        actorsLoading
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign in/i }));

    expect(screen.getByRole('progressbar')).toBeInTheDocument();
    expect(screen.queryByTestId('actor-picker-actor')).not.toBeInTheDocument();
  });
});

describe('<ActorPicker /> — confirmation step', () => {
  it('shows confirm after selecting an actor', async () => {
    const user = userEvent.setup();
    render(
      <ActorPicker
        session={null}
        actors={[ALICE]}
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign in/i }));
    await user.click(await screen.findByTestId('actor-picker-actor'));

    expect(await screen.findByTestId('actor-picker-confirm')).toBeInTheDocument();
    expect(screen.getByTestId('actor-picker-confirm-button')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument();
  });

  it('fires onSignIn with the chosen actor on confirm', async () => {
    const user = userEvent.setup();
    const onSignIn = vi.fn();
    render(
      <ActorPicker
        session={null}
        actors={[ALICE]}
        onSignIn={onSignIn}
        onSignOut={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign in/i }));
    await user.click(await screen.findByTestId('actor-picker-actor'));
    await user.click(screen.getByTestId('actor-picker-confirm-button'));

    expect(onSignIn).toHaveBeenCalledTimes(1);
    expect(onSignIn).toHaveBeenCalledWith(ALICE.actor_id);
  });

  it('cancel returns to the actor list without firing onSignIn', async () => {
    const user = userEvent.setup();
    const onSignIn = vi.fn();
    render(
      <ActorPicker
        session={null}
        actors={[ALICE, BOB]}
        onSignIn={onSignIn}
        onSignOut={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign in/i }));
    await user.click(await screen.findAllByTestId('actor-picker-actor').then((items) => items[0]));
    await user.click(screen.getByRole('button', { name: /cancel/i }));

    // Back to the list, both actors visible.
    expect(screen.queryByTestId('actor-picker-confirm')).not.toBeInTheDocument();
    expect((await screen.findAllByTestId('actor-picker-actor')).length).toBe(2);
    expect(onSignIn).not.toHaveBeenCalled();
  });

  it('disables Confirm while a sign-in is in flight', async () => {
    const user = userEvent.setup();
    render(
      <ActorPicker
        session={null}
        actors={[ALICE]}
        signInPending
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign in/i }));
    await user.click(await screen.findByTestId('actor-picker-actor'));

    const confirm = screen.getByTestId('actor-picker-confirm-button');
    expect(confirm).toBeDisabled();
    expect(confirm).toHaveTextContent(/signing in/i);
  });

  it('surfaces a revoked-key error inline in the popover', async () => {
    const user = userEvent.setup();
    render(
      <ActorPicker
        session={null}
        actors={[ALICE]}
        signInError={{ code: 'key_revoked', message: 'active signing key is revoked' }}
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign in/i }));

    const alerts = await screen.findAllByRole('alert');
    expect(alerts[0]).toHaveTextContent(/revoked/i);
  });
});

describe('<ActorPicker /> — signed in', () => {
  it('renders the truncated actor id and a sign-out button', () => {
    render(
      <ActorPicker
        session={{ actor_id: ALICE.actor_id }}
        actors={[ALICE]}
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );

    expect(screen.getByTestId('actor-picker-signed-in')).toHaveTextContent(
      truncateActorId(ALICE.actor_id),
    );
    expect(screen.getByRole('button', { name: /sign out/i })).toBeInTheDocument();
  });

  it('fires onSignOut when the sign-out button is clicked', async () => {
    const user = userEvent.setup();
    const onSignOut = vi.fn();
    render(
      <ActorPicker
        session={{ actor_id: ALICE.actor_id }}
        actors={[ALICE]}
        onSignIn={() => {}}
        onSignOut={onSignOut}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign out/i }));

    expect(onSignOut).toHaveBeenCalledTimes(1);
  });

  it('disables the sign-out button while a sign-out is in flight', () => {
    render(
      <ActorPicker
        session={{ actor_id: ALICE.actor_id }}
        actors={[ALICE]}
        signOutPending
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );

    const button = screen.getByRole('button', { name: /sign out/i });
    expect(button).toBeDisabled();
    expect(button).toHaveTextContent(/signing out/i);
  });
});

describe('<ActorPicker /> — error clearing', () => {
  it('calls onClearError when the popover closes', async () => {
    const user = userEvent.setup();
    const onClearError = vi.fn();
    render(
      <ActorPicker
        session={null}
        actors={[ALICE]}
        signInError={{ code: 'key_revoked', message: 'key revoked' }}
        onClearError={onClearError}
        onSignIn={() => {}}
        onSignOut={() => {}}
      />,
    );

    await user.click(screen.getByRole('button', { name: /sign in/i }));
    // Close the popover with Escape.
    await user.keyboard('{Escape}');

    expect(onClearError).toHaveBeenCalled();
  });
});

// Avoid unused-import lint when beforeEach goes unused at the
// module level — included as a forward-compat hook for future
// per-test setup.
void beforeEach;
