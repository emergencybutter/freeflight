// Persistence for the plate viewer's highlighter annotations — freehand
// translucent strokes a pilot draws over a SID/STAR/Approach/Airport
// Diagram PDF plate to mark it up (see PlateViewer.tsx). Keyed by the
// plate's own PDF URL, which embeds the FAA d-TPP cycle (e.g.
// `.../d-tpp/2606/00610IL4L.PDF`) — a cycle rollover gives a plate a new
// URL, so last cycle's markup is simply left behind rather than carried
// onto a plate that may have changed, rather than trying to track "the
// same" plate across cycles by procedure identity.
//
// Stored separately from persistence.ts's flight-plan blob for the same
// reason as that file's own STORAGE_KEY comment: different shape, so a
// version bump to one never has to touch the other.
//
// Same best-effort contract as persistence.ts: localStorage can be
// unavailable or hold stale JSON from an older build, so every read falls
// back to an empty result and every write is swallowed on failure.
const STORAGE_KEY = "freeflight.plateAnnotations.v1";

export interface HighlighterStroke {
  id: number;
  /** Hex color, e.g. "#ffe066" — see PlateViewer's HIGHLIGHTER_COLORS. */
  color: string;
  /** [xFraction, yFraction] of the plate's page width/height, in draw
   * order — fractional rather than pixel coordinates so a stroke still
   * lines up correctly after the canvas re-renders at a different size
   * (window resize, "Full Page" toggle). */
  points: [number, number][];
}

type AnnotationsByUrl = Record<string, HighlighterStroke[]>;

function loadAll(): AnnotationsByUrl {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    return parsed && typeof parsed === "object" ? (parsed as AnnotationsByUrl) : {};
  } catch {
    return {};
  }
}

export function loadPlateAnnotations(url: string): HighlighterStroke[] {
  return loadAll()[url] ?? [];
}

/** Drops the entry entirely once a plate's last stroke is erased, rather
 * than leaving an empty array around — keeps the stored blob from slowly
 * accumulating one dead key per plate ever annotated and then cleared. */
export function savePlateAnnotations(url: string, strokes: HighlighterStroke[]): void {
  try {
    const all = loadAll();
    if (strokes.length === 0) {
      delete all[url];
    } else {
      all[url] = strokes;
    }
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch {
    // Storage full or unavailable — persistence is best-effort.
  }
}
