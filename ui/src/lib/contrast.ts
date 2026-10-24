// WCAG 2.x contrast helpers; used by the token test, not by the app.

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Contrast ratio of two `#rrggbb` colors, 1..21. */
export function contrastRatio(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** The `--x: #hex` declarations of the blocks whose selector is exactly `selector`. */
export function tokensOf(
  css: string,
  selector: string,
): Record<string, string> {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const out: Record<string, string> = {};
  for (const block of css.matchAll(
    new RegExp(`(?:^|[\\s,}])${escaped}\\s*\\{([^}]*)\\}`, "g"),
  )) {
    for (const d of block[1].matchAll(
      /(--[\w-]+)\s*:\s*(#[0-9a-fA-F]{6})\s*;/g,
    ))
      out[d[1]] = d[2];
  }
  return out;
}
