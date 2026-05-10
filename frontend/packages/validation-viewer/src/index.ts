// `@kairo/validation-viewer` — components that render the
// daemon's verify-object output. Per `WEB_CLIENT.md` §10/§20
// every status/severity is communicated as text + color, never
// color-only, so the viewer is legible to color-blind users
// and screen readers.

export { ValidationBadge, type ValidationBadgeProps } from './ValidationBadge';
export {
  ValidationIssueList,
  type ValidationIssueListProps,
} from './ValidationIssueList';
