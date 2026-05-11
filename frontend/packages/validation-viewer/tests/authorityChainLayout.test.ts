// Pure-function tests for the authority-chain layout. Same
// design rationale as the statement-graph layout tests: the
// algorithm is the load-bearing piece, the React component is
// just rendering, and a future swap-in (alternate-path
// surfacing, validity-aware chain pruning) lives entirely in
// this file.

import { describe, expect, it } from 'vitest';
import {
  buildAuthorityChains,
  type AuthorityChainInputCapability,
} from '../src/authorityChainLayout';

const ROOT = 'alice';
const BOB = 'bob';
const CAROL = 'carol';
const DAVE = 'dave';

function grant(
  grantor: string,
  grantee: string,
  statement_id: string,
  created_at = '2026-01-01T00:00:00Z',
): AuthorityChainInputCapability {
  return { grantor, grantee, statement_id, created_at };
}

describe('buildAuthorityChains', () => {
  it('returns no chains for an empty capability list', () => {
    expect(buildAuthorityChains(ROOT, [])).toEqual([]);
  });

  it('builds a single-step chain for a direct grant from root', () => {
    const caps = [grant(ROOT, BOB, 's1')];
    const chains = buildAuthorityChains(ROOT, caps);
    expect(chains).toHaveLength(1);
    expect(chains[0]!.steps).toHaveLength(1);
    expect(chains[0]!.steps[0]).toMatchObject({
      grantor: ROOT,
      grantee: BOB,
      statement_id: 's1',
      isFromRoot: true,
    });
    expect(chains[0]!.reachesRoot).toBe(true);
  });

  it('walks back through delegation hops, root-first', () => {
    // Alice → Bob → Carol → Dave
    const caps = [
      grant(ROOT, BOB, 's1'),
      grant(BOB, CAROL, 's2'),
      grant(CAROL, DAVE, 's3'),
    ];
    const chains = buildAuthorityChains(ROOT, caps);
    // One chain per cap; pick the most-grantee'd one (Dave) and
    // verify it walks all the way back.
    const daveChain = chains.find((c) => c.head.grantee === DAVE)!;
    expect(daveChain.reachesRoot).toBe(true);
    expect(daveChain.steps.map((s) => `${s.grantor}->${s.grantee}`)).toEqual([
      `${ROOT}->${BOB}`,
      `${BOB}->${CAROL}`,
      `${CAROL}->${DAVE}`,
    ]);
    // Only the first step has isFromRoot = true.
    expect(daveChain.steps.map((s) => s.isFromRoot)).toEqual([true, false, false]);
  });

  it('marks reachesRoot=false when the chain breaks short of root', () => {
    // Bob granted Carol, but nobody granted Bob the authority —
    // partial / corrupt data.
    const caps = [grant(BOB, CAROL, 's1')];
    const chains = buildAuthorityChains(ROOT, caps);
    expect(chains[0]!.reachesRoot).toBe(false);
    expect(chains[0]!.steps).toHaveLength(1);
    expect(chains[0]!.steps[0]!.isFromRoot).toBe(false);
  });

  it('picks the most recent upstream when multiple candidates exist', () => {
    // Two grantors (alice and someone else) each granted Bob the
    // authority on this object. The chain for Bob's grant to Carol
    // should choose the more recent upstream.
    const otherRoot = 'eve';
    const caps = [
      grant(ROOT, BOB, 'oldGrant', '2026-01-01T00:00:00Z'),
      grant(otherRoot, BOB, 'newGrant', '2026-02-01T00:00:00Z'),
      grant(BOB, CAROL, 'bobToCarol', '2026-03-01T00:00:00Z'),
    ];
    const chains = buildAuthorityChains(ROOT, caps);
    const carolChain = chains.find((c) => c.head.grantee === CAROL)!;
    // Most recent upstream is from `eve`, so the chain doesn't
    // reach our `ROOT` (alice). reachesRoot must be false.
    expect(carolChain.reachesRoot).toBe(false);
    expect(carolChain.steps[0]!.statement_id).toBe('newGrant');
  });

  it('breaks ties on statement_id when timestamps are equal', () => {
    const caps = [
      grant(ROOT, BOB, 'aGrant', '2026-01-01T00:00:00Z'),
      grant(ROOT, BOB, 'zGrant', '2026-01-01T00:00:00Z'),
      grant(BOB, CAROL, 'bobToCarol'),
    ];
    const chains = buildAuthorityChains(ROOT, caps);
    const carolChain = chains.find((c) => c.head.grantee === CAROL)!;
    // Lex-greater statement_id wins.
    expect(carolChain.steps[0]!.statement_id).toBe('zGrant');
  });

  it('detects cycles and stops with reachesRoot=false', () => {
    // Pathological data: Bob granted Carol, Carol granted Bob.
    // Walking up from Carol→Bob→Carol→... must terminate.
    const caps = [grant(BOB, CAROL, 's1'), grant(CAROL, BOB, 's2')];
    const chains = buildAuthorityChains(ROOT, caps);
    // Pick the chain explaining BOB-as-grantee (Carol granted Bob).
    const bobChain = chains.find(
      (c) => c.head.grantee === BOB && c.head.grantor === CAROL,
    )!;
    expect(bobChain.reachesRoot).toBe(false);
    // Should not loop forever; the chain length is bounded by
    // the number of distinct grantees we visit before a repeat.
    expect(bobChain.steps.length).toBeLessThan(10);
  });

  it('returns one chain per input capability head', () => {
    const caps = [grant(ROOT, BOB, 's1'), grant(ROOT, CAROL, 's2')];
    expect(buildAuthorityChains(ROOT, caps)).toHaveLength(2);
  });
});
