// Smart wrapper around `@kairo/ui`'s `<ActorPicker />` — owns
// the TanStack Query hooks and bridges their results into the
// dumb component's props.
//
// Anonymous-by-default per `PHASE_2_WEB_CLIENT_AUTH.md` §1.2:
// while signed out, the picker renders a sign-in button and
// every inspector route continues to function. No route gates
// on `useSession()` in this slice; that comes when the first
// write surface lands.

import { useState } from 'react';
import {
  useKeystoreActors,
  useSession,
  useSignIn,
  useSignOut,
  type KairoApiClientError,
} from '@kairo/api-client';
import { ActorPicker, type ActorPickerError, type ActorPickerProps } from '@kairo/ui';

export function ActorPickerContainer() {
  const sessionQuery = useSession();
  const actorsQuery = useKeystoreActors();
  const signIn = useSignIn();
  const signOut = useSignOut();

  // We keep the *most recent* sign-in error visible across
  // re-renders, and offer the dumb component an
  // `onClearError` to drop it when the popover closes. This
  // avoids a TanStack-Query-driven flash where the mutation
  // error briefly reappears between renders.
  const [stickyError, setStickyError] = useState<ActorPickerError | undefined>(undefined);

  const errorFromMutation = signIn.error !== null ? toPickerError(signIn.error) : undefined;
  const effectiveError = errorFromMutation ?? stickyError;

  const props: ActorPickerProps = {
    session: sessionQuery.data ?? null,
    actors: actorsQuery.data ?? [],
    signInPending: signIn.isPending,
    signOutPending: signOut.isPending,
    actorsLoading: actorsQuery.isPending,
    onSignIn: (actorId) => {
      setStickyError(undefined);
      signIn.mutate(
        { actorId },
        {
          onError: (error) => setStickyError(toPickerError(error)),
        },
      );
    },
    onSignOut: () => {
      signOut.mutate();
    },
    onClearError: () => {
      setStickyError(undefined);
      signIn.reset();
    },
  };

  if (effectiveError !== undefined) {
    props.signInError = effectiveError;
  }

  return <ActorPicker {...props} />;
}

function toPickerError(error: KairoApiClientError): ActorPickerError {
  const detail = error.detail;
  if (detail.kind === 'daemon') {
    return { code: detail.code, message: detail.message };
  }
  if (detail.kind === 'network') {
    return { code: 'network', message: detail.message };
  }
  return { code: 'decode', message: detail.message };
}
