// Pure layout for [`StatementGraphView`](./StatementGraphView).
//
// The deferral note in `specs/PHASE_2_WEB_CLIENT.md` slice 9
// scopes the v1 graph as a vertical timeline — no `dagre`, no
// swim lanes — because the daemon today only emits linear or
// near-linear histories. The layout is one column of nodes
// running top-to-bottom in chronological order (oldest at top,
// matching the `RevisionsTable` ordering on the same page); each
// parent edge is a line from a child node up to its parent node.
//
// Out-of-window parents (a parent revision id that isn't in the
// `revisions` slice — e.g. the caller passes only a tail of a
// long history) get a stub edge pointing up off the top of the
// canvas, with `toIndex = null`. The component renders these
// with a different style so the gap is obvious.
//
// Splitting the layout out from the component keeps the
// algorithm testable without a DOM and makes future swap-ins
// (different layouts, e.g. swim lanes) a one-file change.

/** Suggested per-node and per-edge geometry. Callers can supply
 * their own `LayoutDims` for tight or loose layouts; the defaults
 * match the look used in `ObjectDetail`. */
export interface LayoutDims {
  /** Radius of the node circle, in CSS px. */
  nodeRadius: number;
  /** Vertical spacing between consecutive node centers, in CSS px. */
  rowHeight: number;
  /** X coordinate of every node center (single-column layout). */
  leftMargin: number;
  /** Y coordinate of the first node center. */
  topMargin: number;
}

export const DEFAULT_LAYOUT_DIMS: LayoutDims = {
  nodeRadius: 6,
  rowHeight: 44,
  leftMargin: 24,
  topMargin: 24,
};

/** Minimal subset of `RevisionHeadDto` the layout needs. Spelled
 * out as a structural type so the layout doesn't depend on the
 * DTO definition (and can be exercised against a synthetic
 * fixture in tests). */
export interface GraphInputRevision {
  revision_id: string;
  parents: ReadonlyArray<string>;
}

export interface LayoutNode {
  /** Index into the input `revisions` array. */
  index: number;
  /** Center x coordinate. */
  cx: number;
  /** Center y coordinate. */
  cy: number;
}

export interface LayoutEdge {
  /** Child node — the revision that names this parent. */
  fromIndex: number;
  /** Parent node index, or `null` when the parent isn't in the
   * input slice. The component renders these as a different
   * (dashed) style so the cut is visible. */
  toIndex: number | null;
  /** Source point (child node center). */
  fromX: number;
  fromY: number;
  /** Target point. For an in-window parent this is the parent
   * node's center; for an out-of-window parent it's a stub point
   * one row above the canvas. */
  toX: number;
  toY: number;
}

export interface Layout {
  /** Suggested SVG width — just enough horizontal room for the
   * nodes themselves. The graph's labels get rendered as HTML
   * alongside the SVG, so callers usually wrap this in a flex
   * container and let the label region take the remaining width.
   * Callers that need a stand-alone SVG can pad with their own
   * margin. */
  width: number;
  /** Suggested SVG height — enough room to fit every row plus
   * top/bottom margins. */
  height: number;
  nodes: ReadonlyArray<LayoutNode>;
  edges: ReadonlyArray<LayoutEdge>;
}

export function computeLayout(
  revisions: ReadonlyArray<GraphInputRevision>,
  dims: LayoutDims = DEFAULT_LAYOUT_DIMS,
): Layout {
  const indexByRevisionId = new Map<string, number>();
  revisions.forEach((rev, idx) => {
    indexByRevisionId.set(rev.revision_id, idx);
  });

  const nodes: LayoutNode[] = revisions.map((_, idx) => ({
    index: idx,
    cx: dims.leftMargin,
    cy: dims.topMargin + idx * dims.rowHeight,
  }));

  const edges: LayoutEdge[] = [];
  for (const [childIdx, rev] of revisions.entries()) {
    const childNode = nodes[childIdx]!;
    for (const parentId of rev.parents) {
      const parentIdx = indexByRevisionId.get(parentId);
      if (parentIdx === undefined) {
        // Out-of-window parent — stub edge pointing up off-canvas.
        edges.push({
          fromIndex: childIdx,
          toIndex: null,
          fromX: childNode.cx,
          fromY: childNode.cy,
          toX: childNode.cx,
          toY: dims.topMargin - dims.rowHeight,
        });
      } else {
        const parentNode = nodes[parentIdx]!;
        edges.push({
          fromIndex: childIdx,
          toIndex: parentIdx,
          fromX: childNode.cx,
          fromY: childNode.cy,
          toX: parentNode.cx,
          toY: parentNode.cy,
        });
      }
    }
  }

  // The container only needs room for the node circles — labels
  // live outside the SVG, in a sibling HTML column.
  const width = dims.leftMargin * 2;
  const height =
    revisions.length === 0
      ? dims.topMargin * 2
      : dims.topMargin * 2 + (revisions.length - 1) * dims.rowHeight;

  return { width, height, nodes, edges };
}
