// Per-object authority-chain visualization. Renders one chain
// per capability head, walking back from the head's grantor to
// the object's root authority (the `created_by` actor) via
// upstream grants found in the same capability list.
//
// Per `specs/PHASE_2_WEB_CLIENT.md` slice 9 deferral: this is a
// flat-row visualization over `useCapabilitiesForObject(object)`
// data. The Capability heads table on the same page is the
// source-of-truth tabular surface (one row per grant); this
// view shows the *paths* of authority — answering "why does
// this grantee have this capability?" rather than "what grants
// exist?".
//
// V1 caveats (also documented on `buildAuthorityChains`):
//
//   - Multiple upstream candidates produce one displayed chain;
//     alternate paths aren't surfaced.
//   - The view doesn't validate `is_delegable` or kind matches
//     — it shows the delegation relationships that exist. The
//     daemon's `evaluate_capability` is the source of truth for
//     validity.
//   - When a chain doesn't reach the root authority (broken
//     data, partial fetch, or a cycle), the row carries a
//     visible warning instead of being silently truncated.
//
// Accessibility:
//
//   - The chain list is a `role="list"` with an aria-label
//     summarizing what's drawn.
//   - Each chain row is a `role="listitem"`.
//   - Arrow glyphs between actor boxes are `aria-hidden`; the
//     caller-supplied actor labels carry the meaning.
//   - Empty state is plain text so AT users and sighted users
//     see the same message.

import { Fragment, type ReactNode } from 'react';
import {
  buildAuthorityChains,
  type AuthorityChain,
  type AuthorityChainInputCapability,
} from './authorityChainLayout';

export interface AuthorityChainViewProps<C extends AuthorityChainInputCapability> {
  /** The root authority for this object — typically the
   * object's `created_by` actor. Chains that walk back to this
   * id are marked "reaches root"; chains that don't get a
   * visible warning instead of being silently truncated. */
  rootActor: string;
  /** Cross-actor grants on this object. The component runs the
   * chain-extraction algorithm itself; callers pass the raw
   * `useCapabilitiesForObject` result. */
  capabilities: ReadonlyArray<C>;
  /** Render the actor box for each node. The component is
   * router-agnostic; callers wrap in their own `<Link>` markup
   * so navigation matches the rest of the app. */
  renderActorLabel: (actorId: string) => ReactNode;
  /** Optional: render the edge label between two actor boxes
   * (typically a link to the grant statement). Defaults to a
   * truncated `via <statement-id>` caption. */
  renderGrantEdge?: (statementId: string, createdAt: string) => ReactNode;
  /** What the empty-state placeholder reads. Plain string so AT
   * users and sighted users see the same message. */
  emptyMessage?: string;
}

export function AuthorityChainView<C extends AuthorityChainInputCapability>({
  rootActor,
  capabilities,
  renderActorLabel,
  renderGrantEdge,
  emptyMessage = 'No grants on this object.',
}: AuthorityChainViewProps<C>) {
  if (capabilities.length === 0) {
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

  const chains = buildAuthorityChains(rootActor, capabilities);
  const ariaLabel =
    chains.length === 1
      ? 'Authority chains: 1 chain'
      : `Authority chains: ${chains.length} chains`;

  return (
    <div
      role="list"
      aria-label={ariaLabel}
      style={{ display: 'flex', flexDirection: 'column', rowGap: 12 }}
    >
      {chains.map((chain) => (
        <ChainRow
          key={chain.head.statement_id}
          chain={chain}
          renderActorLabel={renderActorLabel}
          {...(renderGrantEdge !== undefined ? { renderGrantEdge } : {})}
        />
      ))}
    </div>
  );
}

function ChainRow({
  chain,
  renderActorLabel,
  renderGrantEdge,
}: {
  chain: AuthorityChain;
  renderActorLabel: (actorId: string) => ReactNode;
  renderGrantEdge?: (statementId: string, createdAt: string) => ReactNode;
}) {
  // `steps` is non-empty by construction (one step per head),
  // so the leftmost node is steps[0].grantor and each
  // subsequent node is the step's grantee.
  const firstGrantor = chain.steps[0]!.grantor;

  return (
    <div
      role="listitem"
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        flexWrap: 'wrap',
        rowGap: 8,
        columnGap: 4,
      }}
    >
      <ActorBox>{renderActorLabel(firstGrantor)}</ActorBox>
      {chain.steps.map((step) => (
        <Fragment key={step.statement_id}>
          <ChainArrow>
            {renderGrantEdge !== undefined
              ? renderGrantEdge(step.statement_id, step.created_at)
              : `via ${truncate(step.statement_id)}`}
          </ChainArrow>
          <ActorBox>{renderActorLabel(step.grantee)}</ActorBox>
        </Fragment>
      ))}
      {!chain.reachesRoot && <BrokenChainWarning />}
    </div>
  );
}

function ActorBox({ children }: { children: ReactNode }) {
  return (
    <span
      style={{
        display: 'inline-block',
        padding: '4px 10px',
        borderRadius: 6,
        border: '1px solid var(--mui-palette-divider, rgba(0,0,0,0.12))',
        backgroundColor:
          'var(--mui-palette-background-paper, rgba(255,255,255,0.04))',
        // Keep IDs from forcing the row to wrap mid-id.
        whiteSpace: 'nowrap',
      }}
    >
      {children}
    </span>
  );
}

function ChainArrow({ children }: { children: ReactNode }) {
  return (
    <span
      style={{
        display: 'inline-flex',
        flexDirection: 'column',
        alignItems: 'center',
        // Vertical breathing room between the arrow glyph and the label.
        rowGap: 2,
        margin: '0 4px',
        color: 'var(--mui-palette-text-secondary, #666)',
      }}
    >
      <span aria-hidden="true" style={{ fontSize: '1.1em', lineHeight: 1 }}>
        →
      </span>
      <span style={{ fontSize: '0.75em' }}>{children}</span>
    </span>
  );
}

function BrokenChainWarning() {
  return (
    <span
      style={{
        marginLeft: 8,
        padding: '2px 8px',
        borderRadius: 4,
        fontSize: '0.75em',
        color: 'var(--mui-palette-warning-contrastText, #000)',
        backgroundColor: 'var(--mui-palette-warning-light, #ffe9b3)',
      }}
    >
      Does not reach root authority
    </span>
  );
}

function truncate(s: string): string {
  if (s.length <= 16) return s;
  return `${s.slice(0, 8)}…${s.slice(-4)}`;
}
