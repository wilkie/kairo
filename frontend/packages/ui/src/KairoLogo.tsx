// Brand marks. Both render the canonical SVGs from
// `./assets/`, so any size scales without quality loss. Use
// `<KairoLogo>` for primary placement (drawer, splash, login)
// and `<KairoIcon>` for compact contexts (favicon-like rails,
// dense headers).

import logoUrl from './assets/kairo_logo_full_padded.svg?url';
import iconUrl from './assets/kairo_icon.svg?url';

export interface KairoLogoProps {
  /** Rendered height in CSS pixels. Width scales to preserve
   * the SVG's intrinsic aspect ratio. Defaults to 32. */
  height?: number;
  /** Optional accessible label. The default is "Kairo"; pass an
   * empty string to mark the logo as decorative when paired
   * with adjacent text. */
  alt?: string;
}

export function KairoLogo({ height = 32, alt = 'Kairo' }: KairoLogoProps) {
  return (
    <img
      src={logoUrl}
      alt={alt}
      height={height}
      style={{ height, width: 'auto', display: 'block' }}
    />
  );
}

export function KairoIcon({ height = 32, alt = 'Kairo' }: KairoLogoProps) {
  return (
    <img
      src={iconUrl}
      alt={alt}
      height={height}
      width={height}
      style={{ height, width: height, display: 'block' }}
    />
  );
}
