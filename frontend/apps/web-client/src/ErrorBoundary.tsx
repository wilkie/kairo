// Top-level error boundary. Catches unexpected component
// throws (anything not surfaced through TanStack Query's
// `error` channel — e.g., a bug in render, a synchronous throw
// in a layout component) and renders the structured
// `ErrorDisplay`.
//
// React 19 still requires a class component for error
// boundaries. Functional `useError()` is on the roadmap but
// not shipped, so this stays a class for v1.

import { Component, type ErrorInfo, type ReactNode } from 'react';
import { Button, ErrorDisplay } from '@kairo/ui';

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    // Surface unexpected throws on the dev console; production
    // should ship them to a logging endpoint, but the inspector
    // doesn't have one yet.
    console.error('Inspector caught an unexpected render error', error, info);
  }

  private handleReset = () => {
    this.setState({ error: null });
  };

  override render(): ReactNode {
    const { error } = this.state;
    if (error === null) {
      return this.props.children;
    }
    return (
      <div style={{ padding: '2rem', maxWidth: '40rem', margin: '0 auto' }}>
        <ErrorDisplay
          title="Something went wrong"
          message="The inspector encountered an unexpected error while rendering."
          detail={error.message}
          actions={
            <Button variant="primary" onClick={this.handleReset}>
              Try again
            </Button>
          }
        />
      </div>
    );
  }
}
