// Unit tests for `<ProvenanceBadge />`.

import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { ProvenanceBadge, peerLabel } from '../src/ProvenanceBadge';

const ARCHIVE = {
  peerUrl: 'https://archive.example.org',
  peerActor: 'zMockActorCarol00000000000000000000000000',
  fetchedAt: '2026-10-01T12:00:00Z',
};

afterEach(() => {
  cleanup();
});

describe('<ProvenanceBadge />', () => {
  it('renders nothing for a local record', () => {
    const { container } = render(<ProvenanceBadge entries={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('names the first peer', () => {
    render(<ProvenanceBadge entries={[ARCHIVE]} />);
    expect(screen.getByText('Fetched from archive.example.org')).toBeInTheDocument();
    const badge = screen.getByTestId('provenance-badge');
    expect(badge).toHaveAccessibleName(/authenticated/);
    expect(badge).toHaveAccessibleName(/2026-10-01T12:00:00Z/);
  });

  it('counts further peers', () => {
    render(
      <ProvenanceBadge
        entries={[
          ARCHIVE,
          { ...ARCHIVE, peerUrl: 'https://mirror.example.net', fetchedAt: '2026-10-02T08:30:00Z' },
        ]}
      />,
    );
    expect(screen.getByText('Fetched from archive.example.org +1')).toBeInTheDocument();
    expect(screen.getByTestId('provenance-badge')).toHaveAccessibleName(/mirror\.example\.net/);
  });

  it('strips the scheme for the label', () => {
    expect(peerLabel('http://127.0.0.1:7880')).toBe('127.0.0.1:7880');
    expect(peerLabel('https://a.example:8443')).toBe('a.example:8443');
  });
});
