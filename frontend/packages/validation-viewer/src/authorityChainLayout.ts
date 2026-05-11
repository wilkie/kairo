// Pure layout for [`AuthorityChainView`](./AuthorityChainView).
//
// For an object, "authority" flows from the root authority (the
// object's `created_by` actor) outward through capability grants.
// `list_capabilities_for_object` returns a flat list of grants;
// this module walks that list backward from each grant's grantor
// to reconstruct the chain that authorizes the head.
//
// The algorithm: for each capability head, start a chain at the
// head itself, then repeatedly look for an upstream grant where
// `grantee == current.grantor` until we either reach the root
// actor or run out of upstream candidates. The chain is then
// rendered grantor→grantee, root-first.
//
// V1 simplifications (documented so callers know what they're
// looking at):
//
//   - Multiple upstream candidates: when more than one grant
//     would qualify as "the upstream that authorized this
//     grantor", we pick the most recent by `(created_at,
//     statement_id)` descending — the same tiebreak rule
//     `chain_head` uses for chain leaves elsewhere. This means
//     the rendered chain shows *one* path even when several
//     valid paths exist; alternate-path rendering is a
//     post-MVP follow-up.
//   - `is_delegable` and per-grant kinds aren't on
//     `CapabilityHeadDto`. The chain view therefore *displays*
//     the delegation relationships that exist without claiming
//     the chain is valid (a grantor might have re-granted
//     beyond their delegation depth, or for kinds they don't
//     hold). The daemon's `evaluate_capability` is the source
//     of truth for validity; this view is for orientation.
//   - Cycle detection: if the same grantor appears twice while
//     walking up, we stop and mark the chain `reachesRoot:
//     false`. Cycles shouldn't happen against a well-formed
//     store, but we'd rather render a truncated chain than
//     loop forever on bad data.
//
// Splitting the layout out from the component keeps the
// algorithm testable without a DOM and isolates future changes
// (e.g. surfacing alternate paths) to one file.

/** Minimal subset of `CapabilityHeadDto` the layout needs.
 * Spelled out as a structural type so the layout doesn't depend
 * on the DTO definition (and can be exercised against a
 * synthetic fixture in tests). */
export interface AuthorityChainInputCapability {
  grantor: string;
  grantee: string;
  statement_id: string;
  /** RFC 3339; used as the `(created_at, statement_id)`
   * tiebreak when selecting an upstream candidate. */
  created_at: string;
}

/** One step in an authority chain — i.e. one grant. */
export interface AuthorityChainStep {
  grantor: string;
  grantee: string;
  statement_id: string;
  created_at: string;
  /** True if `grantor` is the root authority (the object's
   * creator). The leftmost step in a chain that `reachesRoot`
   * has this set; intermediate steps don't. */
  isFromRoot: boolean;
}

/** Full chain explaining a single capability head. */
export interface AuthorityChain {
  /** The head this chain explains. Useful for keying and label
   * rendering; the head is also the last element of `steps`. */
  head: AuthorityChainInputCapability;
  /** Steps from root authority outward to the head's grantee.
   * Each step's `grantee` matches the next step's `grantor`. The
   * last step's `grantee` matches `head.grantee`. */
  steps: ReadonlyArray<AuthorityChainStep>;
  /** True if the chain successfully terminates at the root
   * authority. False if walking upstream ran out of candidates,
   * or if a cycle was detected. */
  reachesRoot: boolean;
}

export function buildAuthorityChains(
  rootActor: string,
  capabilities: ReadonlyArray<AuthorityChainInputCapability>,
): AuthorityChain[] {
  return capabilities.map((head) => buildChainFor(rootActor, head, capabilities));
}

function buildChainFor(
  rootActor: string,
  head: AuthorityChainInputCapability,
  all: ReadonlyArray<AuthorityChainInputCapability>,
): AuthorityChain {
  const stepsReversed: AuthorityChainStep[] = [];
  let current: AuthorityChainInputCapability = head;
  // Track grantors we've already used as the "grantee position"
  // of a chain step. Revisiting one means we hit a cycle.
  const visitedGrantees = new Set<string>();
  let reachesRoot = false;

  while (true) {
    const isFromRoot = current.grantor === rootActor;
    stepsReversed.push({
      grantor: current.grantor,
      grantee: current.grantee,
      statement_id: current.statement_id,
      created_at: current.created_at,
      isFromRoot,
    });
    visitedGrantees.add(current.grantee);

    if (isFromRoot) {
      reachesRoot = true;
      break;
    }

    // The grantor needs an upstream grant where they were the
    // grantee. Pick the most recent by (created_at,
    // statement_id) descending — same tiebreak rule the store
    // uses for chain leaves elsewhere.
    const candidates = all.filter((c) => c.grantee === current.grantor);
    if (candidates.length === 0) {
      // Broken chain — grantor isn't the root and nobody granted
      // them authority on this object.
      break;
    }
    const upstream = pickMostRecent(candidates);
    if (visitedGrantees.has(upstream.grantee)) {
      // Cycle — stop here and report the chain as not reaching root.
      break;
    }
    current = upstream;
  }

  // We pushed steps from grantee outward; reverse for display.
  return {
    head,
    steps: stepsReversed.reverse(),
    reachesRoot,
  };
}

function pickMostRecent(
  candidates: ReadonlyArray<AuthorityChainInputCapability>,
): AuthorityChainInputCapability {
  // Caller guarantees non-empty.
  return candidates.reduce((best, c) => {
    if (c.created_at > best.created_at) return c;
    if (c.created_at < best.created_at) return best;
    return c.statement_id > best.statement_id ? c : best;
  });
}
