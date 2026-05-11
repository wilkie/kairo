// Vertical-timeline graph of an object's revisions. The
// inspector renders this alongside the `RevisionsTable` on the
// object detail page; the table is the source-of-truth tabular
// surface, the graph is a visual aid for parent relationships.
//
// Per `specs/PHASE_2_WEB_CLIENT.md` slice 9 deferral: this is an
// SVG over `useRevisions(object)` data with no `dagre` dep.
// One column, oldest at top, child→parent edges drawn as
// straight vertical lines. Long edges (across multiple rows) are
// covered by intermediate node circles via SVG z-order; for the
// linear / near-linear histories the daemon emits today, that's
// a clean read.
//
// Accessibility:
//
//   - The SVG is decorative (`aria-hidden`); screen readers see
//     the labels in the sibling column instead, which carry the
//     same data and any caller-supplied `<Link>` markup.
//   - The component still exposes a `role="figure"` wrapper with
//     an `aria-label` summarizing what's drawn, so AT users get
//     a one-line summary before they reach the labels.
//   - Empty state is rendered as plain text (no SVG) so AT users
//     and sighted users see the same message.

import { type ReactNode } from 'react';
import {
  DEFAULT_LAYOUT_DIMS,
  computeLayout,
  type GraphInputRevision,
  type LayoutDims,
} from './statementGraphLayout';

export interface StatementGraphViewRevision extends GraphInputRevision {
  /** Statement id used as the row key. The graph never displays
   * this directly — it just needs a stable React key per row. */
  statement_id: string;
}

export interface StatementGraphViewProps<R extends StatementGraphViewRevision> {
  /** Revisions to draw, in chronological order (oldest first).
   * The component does not re-sort; it renders in the order it
   * receives. */
  revisions: ReadonlyArray<R>;
  /** Render the label column for one revision. The component is
   * router-agnostic: callers wrap each label in their own
   * `<Link>` so navigation matches the rest of the app. */
  renderLabel: (revision: R) => ReactNode;
  /** Optional layout overrides. Defaults to
   * [`DEFAULT_LAYOUT_DIMS`]. */
  dims?: LayoutDims;
  /** What the empty-state placeholder reads. Plain string so AT
   * users and sighted users see the same message. */
  emptyMessage?: string;
}

export function StatementGraphView<R extends StatementGraphViewRevision>({
  revisions,
  renderLabel,
  dims = DEFAULT_LAYOUT_DIMS,
  emptyMessage = 'No revisions to graph.',
}: StatementGraphViewProps<R>) {
  if (revisions.length === 0) {
    return (
      <p
        style={{
          margin: 0,
          fontStyle: 'italic',
          color: 'var(--mui-palette-text-secondary, #666)',
        }}
      >
        {emptyMessage}
      </p>
    );
  }

  const layout = computeLayout(revisions, dims);
  const ariaLabel =
    revisions.length === 1
      ? 'Revision graph: 1 revision'
      : `Revision graph: ${revisions.length} revisions`;

  return (
    <div
      role="figure"
      aria-label={ariaLabel}
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        width: '100%',
        // Reserve enough vertical space for the SVG so the label
        // column's absolutely-positioned rows have somewhere to
        // land.
        minHeight: layout.height,
        position: 'relative',
      }}
    >
      <svg
        width={layout.width}
        height={layout.height}
        aria-hidden="true"
        style={{ flexShrink: 0, overflow: 'visible' }}
      >
        {layout.edges.map((edge, i) => (
          <line
            // Edge keys: child index + parent index uniquely
            // identifies an edge (out-of-window parents become
            // null, but a child has at most one out-of-window
            // parent in practice; if it has more, the index
            // suffix disambiguates).
            key={`edge-${edge.fromIndex}-${edge.toIndex ?? 'stub'}-${i}`}
            x1={edge.fromX}
            y1={edge.fromY}
            x2={edge.toX}
            y2={edge.toY}
            stroke="currentColor"
            strokeOpacity={edge.toIndex === null ? 0.4 : 0.6}
            strokeWidth={1.5}
            strokeDasharray={edge.toIndex === null ? '4 3' : undefined}
          />
        ))}
        {layout.nodes.map((node) => (
          <circle
            key={`node-${node.index}`}
            cx={node.cx}
            cy={node.cy}
            r={dims.nodeRadius}
            fill="currentColor"
            fillOpacity={0.9}
          />
        ))}
      </svg>

      <div
        style={{
          flex: 1,
          position: 'relative',
          minHeight: layout.height,
          marginLeft: 8,
        }}
      >
        {revisions.map((rev, i) => {
          const node = layout.nodes[i]!;
          return (
            <div
              key={rev.statement_id}
              style={{
                position: 'absolute',
                // Vertically center the label on the node.
                top: node.cy - dims.rowHeight / 2,
                left: 0,
                right: 0,
                height: dims.rowHeight,
                display: 'flex',
                alignItems: 'center',
              }}
            >
              {renderLabel(rev)}
            </div>
          );
        })}
      </div>
    </div>
  );
}
