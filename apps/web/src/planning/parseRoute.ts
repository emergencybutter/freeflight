/**
 * Route string parsing and normalization for FreeFlight.
 * Handles ICAO/FAA flight plan route formats, e.g.:
 * "KMMU/05 N0173F040 DCT PIYIN V3 SBJ V419 BIGGY V3 MXE V419 EMI V166 MRB/N0178F060 V44 MGW V75 AIR V103 ATWOO DCT KMNN/13"
 */

// Matches speed + altitude/level tokens, e.g. N0173F040, N0150A045, M082F370, K0800F350, N0173VFR
const SPEED_ALT_RE = /^[NKM]\d{3,4}(?:[FA]\d{3}|[SM]\d{4}|VFR)$/i;

// Matches standalone speed or flight level, e.g. FL040, FL350, N0173, M082
const SPEED_OR_FL_RE = /^(?:FL\d{3}|[NKM]\d{3,4})$/i;

// Matches altitude in feet or pure numbers, e.g. 4000FT, 10000FT, 4000, 05
const ALT_OR_NUMERIC_RE = /^\d+(?:FT)?$/i;

/**
 * Splits and cleans a raw route string into an array of uppercase navigational idents.
 *
 * Strips:
 * - Outer quotes ("..." or '...')
 * - Delimiters (whitespace, commas, and dots like SimBrief's "..")
 * - Direct routing indicators ("DCT", "DIRECT")
 * - Standalone flight rule indicators ("VFR", "IFR")
 * - Speed/altitude declarations (e.g. "N0173F040", "FL040", "4000FT")
 * - Slashes and trailing modifiers on idents (e.g. "KMMU/05" -> "KMMU", "MRB/N0178F060" -> "MRB", "KMNN/13" -> "KMNN")
 * - Stray tokens starting with "/" or pure numeric tokens
 */
export function cleanRouteTokens(raw: string): string[] {
  let cleaned = raw.trim().replace(/^["']|["']$/g, "").trim();
  if (!cleaned) return [];

  // Collapse whitespace around slashes so e.g. "KMMU / 05" -> "KMMU/05"
  cleaned = cleaned.replace(/\s*\/\s*/g, "/");

  // Replace dots (e.g. ".." or ".") and commas with spaces
  cleaned = cleaned.replace(/[\s,]+/g, " ").replace(/\.+/g, " ");

  const rawParts = cleaned.split(/\s+/);
  const tokens: string[] = [];

  for (const part of rawParts) {
    if (!part || part.startsWith("/")) continue;
    // Extract ident before any slash (e.g. KMMU/05 -> KMMU, MRB/N0178F060 -> MRB)
    const base = part.split("/")[0].toUpperCase().trim();
    if (!base) continue;

    // Filter out non-navigational tokens
    if (base === "DCT" || base === "DIRECT") continue;
    if (base === "VFR" || base === "IFR") continue;
    if (SPEED_ALT_RE.test(base) || SPEED_OR_FL_RE.test(base) || ALT_OR_NUMERIC_RE.test(base)) continue;

    tokens.push(base);
  }

  return tokens;
}

/**
 * Determines whether a string input looks like a multi-token route string
 * rather than a single fix/navaid search query.
 */
export function isRouteString(input: string): boolean {
  const trimmed = input.trim().replace(/^["']|["']$/g, "").trim();
  if (!trimmed) return false;
  // If it contains spaces, commas, dots, or a slash with a route/runway modifier (e.g. KMMU/05)
  if (/[\s,]|\.\.|\/\d|\/[NKM]/.test(trimmed)) {
    const tokens = cleanRouteTokens(trimmed);
    return tokens.length >= 2 || (trimmed.includes("/") && tokens.length >= 1);
  }
  return cleanRouteTokens(trimmed).length >= 2;
}
