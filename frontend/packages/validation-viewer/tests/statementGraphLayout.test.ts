// Pure-function tests for the statement-graph layout. The
// component renders these layouts to SVG; testing the layout in
// isolation is faster than spinning up React, and it locks the
// algorithm contract so a future swap-in (e.g. a multi-column
// swim-lane layout) is a focused change in one file.

import { describe, expect, it } from 'vitest';
import {
  DEFAULT_LAYOUT_DIMS,
  computeLayout,
  type GraphInputRevision,
} from '../src/statementGraphLayout';

function rev(id: string, parents: string[] = []): GraphInputRevision {
  return { revision_id: id, parents };
}

describe('computeLayout', () => {
  it('returns no nodes or edges for an empty input', () => {
    const layout = computeLayout([]);
    expect(layout.nodes).toEqual([]);
    expect(layout.edges).toEqual([]);
    // Even an empty graph leaves room for the canonical top/bottom
    // margins so the panel doesn't collapse to zero height.
    expect(layout.height).toBe(DEFAULT_LAYOUT_DIMS.topMargin * 2);
  });

  it('places a single revision at the top margin with no edges', () => {
    const layout = computeLayout([rev('a')]);
    expect(layout.nodes).toHaveLength(1);
    expect(layout.nodes[0]).toMatchObject({
      index: 0,
      cx: DEFAULT_LAYOUT_DIMS.leftMargin,
      cy: DEFAULT_LAYOUT_DIMS.topMargin,
    });
    expect(layout.edges).toEqual([]);
  });

  it('lays out a linear three-revision chain top-to-bottom', () => {
    const layout = computeLayout([rev('a'), rev('b', ['a']), rev('c', ['b'])]);
    expect(layout.nodes).toHaveLength(3);
    // Each row sits one rowHeight below the previous one.
    expect(layout.nodes[1]!.cy - layout.nodes[0]!.cy).toBe(DEFAULT_LAYOUT_DIMS.rowHeight);
    expect(layout.nodes[2]!.cy - layout.nodes[1]!.cy).toBe(DEFAULT_LAYOUT_DIMS.rowHeight);
    // Two edges: b→a and c→b. Each goes from the child node up
    // to its parent.
    expect(layout.edges).toHaveLength(2);
    expect(layout.edges[0]).toMatchObject({ fromIndex: 1, toIndex: 0 });
    expect(layout.edges[0]!.fromY).toBe(layout.nodes[1]!.cy);
    expect(layout.edges[0]!.toY).toBe(layout.nodes[0]!.cy);
    expect(layout.edges[1]).toMatchObject({ fromIndex: 2, toIndex: 1 });
  });

  it('emits a stub edge with toIndex=null for an out-of-window parent', () => {
    // The caller passes only the tail of a longer history; the
    // first revision names a parent we don't know about.
    const layout = computeLayout([rev('b', ['outside']), rev('c', ['b'])]);
    expect(layout.edges).toHaveLength(2);
    const stub = layout.edges[0]!;
    expect(stub.fromIndex).toBe(0);
    expect(stub.toIndex).toBeNull();
    // Stub points up off the top of the canvas so the gap is visible.
    expect(stub.toY).toBeLessThan(DEFAULT_LAYOUT_DIMS.topMargin);
  });

  it('emits one edge per parent on a merge revision', () => {
    // c is a merge of a and b.
    const layout = computeLayout([rev('a'), rev('b'), rev('c', ['a', 'b'])]);
    const cEdges = layout.edges.filter((e) => e.fromIndex === 2);
    expect(cEdges).toHaveLength(2);
    expect(cEdges.map((e) => e.toIndex).sort()).toEqual([0, 1]);
  });

  it('emits one edge per child on a forked parent', () => {
    // Both b and c are children of a (independent forks).
    const layout = computeLayout([rev('a'), rev('b', ['a']), rev('c', ['a'])]);
    const aIncoming = layout.edges.filter((e) => e.toIndex === 0);
    expect(aIncoming).toHaveLength(2);
    expect(aIncoming.map((e) => e.fromIndex).sort()).toEqual([1, 2]);
  });

  it('honors caller-supplied dims', () => {
    const layout = computeLayout([rev('a'), rev('b', ['a'])], {
      nodeRadius: 4,
      rowHeight: 20,
      leftMargin: 10,
      topMargin: 5,
    });
    expect(layout.nodes[0]).toMatchObject({ cx: 10, cy: 5 });
    expect(layout.nodes[1]).toMatchObject({ cx: 10, cy: 25 });
  });

  it('produces an height that fits every row', () => {
    const layout = computeLayout([rev('a'), rev('b', ['a']), rev('c', ['b'])]);
    // height = topMargin * 2 + (n-1) * rowHeight
    const expected =
      DEFAULT_LAYOUT_DIMS.topMargin * 2 + 2 * DEFAULT_LAYOUT_DIMS.rowHeight;
    expect(layout.height).toBe(expected);
  });
});
