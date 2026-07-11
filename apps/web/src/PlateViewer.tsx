// Renders a single FAA d-TPP plate (SID/STAR/Approach chart or Airport
// Diagram) and lets a pilot mark it up with translucent highlighter
// strokes — the feature this component exists for. A plain
// `<iframe src="https://aeronav.faa.gov/...">` (the previous approach,
// still how QuickChartLinks/ProcedurePanel/AirportDiagramPanel used to
// show these) can't support that: annotating means drawing on pixels we
// control, and a cross-origin iframe's rendered PDF content is opaque to
// this page's JS. So this renders the PDF itself via pdf.js onto a
// `<canvas>`, with a second, transparent canvas stacked on top for the
// highlighter strokes.
//
// pdf.js needs to fetch the PDF's bytes itself (an iframe's `src`
// navigation doesn't go through page JS at all, so it never hit this
// problem) — aeronav.faa.gov sends no CORS headers, so a direct
// cross-origin `fetch()` from this page is blocked. `ff-api`'s
// `/dtpp/plate?url=` proxy (services/ff-api/src/routes/dtpp.rs) fetches
// server-side instead, where CORS doesn't apply, and re-serves the same
// bytes from our own origin.
import { useEffect, useRef, useState } from "react";
import * as pdfjsLib from "pdfjs-dist";
import { API_BASE_URL } from "./api";
import { loadPlateAnnotations, savePlateAnnotations, type HighlighterStroke } from "./annotations";

// See pdf.js's own bundler docs — this is the standard way to point it at
// its worker script under Vite/webpack without extra config, since a
// worker can't be resolved via a plain bare specifier.
pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.min.mjs",
  import.meta.url,
).toString();

// Picked to read as classic highlighter-pen colors once drawn translucent
// (see ANNOTATION_OPACITY below), not as fully-saturated UI accents.
const HIGHLIGHTER_COLORS = [
  { id: "yellow", color: "#ffe066" },
  { id: "green", color: "#7ee787" },
  { id: "pink", color: "#ff6fae" },
  { id: "blue", color: "#6fd3ff" },
] as const;

const ANNOTATION_LINE_WIDTH_PX = 16;
const ANNOTATION_OPACITY = 0.4;
// Minimum movement (CSS px) between sampled points while dragging — keeps
// a freehand stroke's point count sane without visibly faceting the line
// at normal draw speed.
const SAMPLE_MIN_PX = 3;
// A raw pointer position easily lands a few px off the (already only
// ANNOTATION_LINE_WIDTH_PX-wide) line it's meant to erase.
const ERASER_HIT_RADIUS_PX = 12;

// Full-page zoom (§9.1): 1 = fit-to-viewport (the inline size's only
// option too), up to MAX_ZOOM. Only offered in the expanded overlay —
// the inline plate is always fit. Zooming past fit re-renders the PDF at
// the higher resolution (sharp, not a CSS upscale) and lets the overlay
// scroll to pan.
const MIN_ZOOM = 1;
const MAX_ZOOM = 5;
const ZOOM_STEP = 1.25;

function distance(ax: number, ay: number, bx: number, by: number): number {
  return Math.hypot(ax - bx, ay - by);
}

/** Shortest distance from point `p` to the segment `a`-`b`, all in the
 * same (CSS-px) space — used by the eraser to hit-test a stroke's
 * polyline rather than just its individual sampled points, so a fast,
 * coarsely-sampled stroke doesn't have erasable "gaps" between points. */
function pointToSegmentDistance(
  px: number,
  py: number,
  ax: number,
  ay: number,
  bx: number,
  by: number,
): number {
  const dx = bx - ax;
  const dy = by - ay;
  const lenSq = dx * dx + dy * dy;
  if (lenSq === 0) return distance(px, py, ax, ay);
  let t = ((px - ax) * dx + (py - ay) * dy) / lenSq;
  t = Math.max(0, Math.min(1, t));
  return distance(px, py, ax + t * dx, ay + t * dy);
}

function hitTestStroke(points: [number, number][], px: number, py: number, hitRadiusPx: number): boolean {
  if (points.length === 1) return distance(px, py, points[0][0], points[0][1]) <= hitRadiusPx;
  for (let i = 0; i < points.length - 1; i++) {
    if (pointToSegmentDistance(px, py, points[i][0], points[i][1], points[i + 1][0], points[i + 1][1]) <= hitRadiusPx) {
      return true;
    }
  }
  return false;
}

function drawStroke(ctx: CanvasRenderingContext2D, points: [number, number][], color: string) {
  if (points.length < 2) return;
  ctx.strokeStyle = color;
  ctx.globalAlpha = ANNOTATION_OPACITY;
  ctx.lineWidth = ANNOTATION_LINE_WIDTH_PX;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(points[0][0], points[0][1]);
  for (const [x, y] of points.slice(1)) ctx.lineTo(x, y);
  ctx.stroke();
}

interface PlateViewerProps {
  url: string;
  title: string;
  expanded: boolean;
  onToggleExpanded: () => void;
  /** "Reduce" (default) toggles back to the inline size; QuickChartLinks'
   * overlay has no inline size to return to, so it passes "Close" instead
   * — same handler either way, just a clearer label for that context. */
  collapseLabel?: string;
}

export function PlateViewer({ url, title, expanded, onToggleExpanded, collapseLabel }: PlateViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const pageWrapRef = useRef<HTMLDivElement>(null);
  const pdfCanvasRef = useRef<HTMLCanvasElement>(null);
  const drawCanvasRef = useRef<HTMLCanvasElement>(null);
  const pageRef = useRef<pdfjsLib.PDFPageProxy | null>(null);
  const renderTaskRef = useRef<pdfjsLib.RenderTask | null>(null);
  const cssSizeRef = useRef({ width: 0, height: 0 });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // Full-page zoom factor over fit-to-viewport (see MIN_ZOOM). Held in a
  // ref too because renderPage runs from the mount-time ResizeObserver
  // closure, which must read the current zoom rather than a stale one.
  const [zoom, setZoom] = useState(MIN_ZOOM);
  const zoomRef = useRef(MIN_ZOOM);

  const [strokes, setStrokes] = useState<HighlighterStroke[]>(() => loadPlateAnnotations(url));
  const [activeColor, setActiveColor] = useState<string | null>(null);
  const [eraseActive, setEraseActive] = useState(false);
  const activeColorRef = useRef<string | null>(null);
  const eraseActiveRef = useRef(false);
  const isDrawingRef = useRef(false);
  const isErasingRef = useRef(false);
  const draftPointsRef = useRef<[number, number][]>([]);
  const nextIdRef = useRef(1);

  useEffect(() => {
    activeColorRef.current = activeColor;
  }, [activeColor]);
  useEffect(() => {
    eraseActiveRef.current = eraseActive;
  }, [eraseActive]);

  // Redraws every committed stroke (plus whatever's mid-drag) into the
  // overlay canvas, in its current CSS-pixel size. Called after every
  // strokes/resize change rather than incrementally patching the canvas —
  // a full redraw is cheap here (a handful of strokes, plain 2D draw
  // calls), unlike MapLibre's GeoJSON-diffing sources in MapView.
  function redraw() {
    const canvas = drawCanvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.restore();
    const { width, height } = cssSizeRef.current;
    for (const s of strokes) {
      drawStroke(
        ctx,
        s.points.map(([fx, fy]): [number, number] => [fx * width, fy * height]),
        s.color,
      );
    }
    if (draftPointsRef.current.length >= 2 && activeColorRef.current) {
      drawStroke(ctx, draftPointsRef.current, activeColorRef.current);
    }
  }

  // Persists on every change and keeps the overlay in sync — mirrors
  // persistence.ts's best-effort localStorage write.
  useEffect(() => {
    savePlateAnnotations(url, strokes);
    redraw();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [strokes]);

  // Switching plates (a different SID vs. STAR, say) loads that plate's
  // own strokes fresh rather than carrying the previous plate's markup
  // over onto a different chart, and drops any zoom.
  useEffect(() => {
    const loaded = loadPlateAnnotations(url);
    setStrokes(loaded);
    nextIdRef.current = loaded.reduce((max, s) => Math.max(max, s.id), 0) + 1;
    setActiveColor(null);
    setEraseActive(false);
    setZoom(MIN_ZOOM);
  }, [url]);

  // Collapsing back to the inline plate drops any zoom — the inline size
  // is always fit, so a leftover zoom would render it overflowing its
  // fixed-height box.
  useEffect(() => {
    if (!expanded) setZoom(MIN_ZOOM);
  }, [expanded]);

  // Keep zoomRef in sync and re-render at the new zoom whenever it
  // changes (renderPage reads zoomRef, not the `zoom` state, since it
  // also runs from the mount-time ResizeObserver closure).
  useEffect(() => {
    zoomRef.current = zoom;
    renderPage();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [zoom]);

  // Fits the page (contain — whole page visible) into the current
  // container box, times the current zoom (1 = fit; full-page only), at
  // devicePixelRatio resolution for sharpness, and re-renders both
  // canvases at that size. Called on load, on every container resize
  // (window resize, the Full Page toggle), on zoom change, and
  // is idempotent — safe to call repeatedly.
  function renderPage() {
    const page = pageRef.current;
    const container = containerRef.current;
    const pageWrap = pageWrapRef.current;
    const pdfCanvas = pdfCanvasRef.current;
    const drawCanvas = drawCanvasRef.current;
    if (!page || !container || !pageWrap || !pdfCanvas || !drawCanvas) return;

    const unscaled = page.getViewport({ scale: 1 });
    const containerWidth = container.clientWidth;
    const containerHeight = container.clientHeight;
    if (containerWidth === 0 || containerHeight === 0) return;
    // Fit-to-container, then the user's zoom on top (1 = fit). At zoom > 1
    // the page grows past the container and the overlay scrolls to pan.
    const fitScale = Math.min(containerWidth / unscaled.width, containerHeight / unscaled.height);
    const scale = fitScale * zoomRef.current;
    const cssWidth = unscaled.width * scale;
    const cssHeight = unscaled.height * scale;
    cssSizeRef.current = { width: cssWidth, height: cssHeight };

    pageWrap.style.width = `${cssWidth}px`;
    pageWrap.style.height = `${cssHeight}px`;

    // Rasterize at the zoomed scale × dpr so zooming in stays crisp
    // (a real re-render at higher resolution, not a blurry CSS upscale).
    const dpr = window.devicePixelRatio || 1;
    const viewport = page.getViewport({ scale: scale * dpr });

    pdfCanvas.width = viewport.width;
    pdfCanvas.height = viewport.height;
    pdfCanvas.style.width = `${cssWidth}px`;
    pdfCanvas.style.height = `${cssHeight}px`;
    drawCanvas.width = cssWidth * dpr;
    drawCanvas.height = cssHeight * dpr;
    drawCanvas.style.width = `${cssWidth}px`;
    drawCanvas.style.height = `${cssHeight}px`;
    const drawCtx = drawCanvas.getContext("2d");
    drawCtx?.setTransform(dpr, 0, 0, dpr, 0, 0);

    // A resize can fire again before the previous render finishes (e.g. a
    // window drag-resize, or two ResizeObserver callbacks back to back
    // from the Full Page toggle) — pdf.js only allows one render task per
    // page at a time, so cancel whichever's still in flight before
    // starting the new one rather than letting it reject with "already
    // rendering".
    renderTaskRef.current?.cancel();
    const pdfCtx = pdfCanvas.getContext("2d");
    if (pdfCtx) {
      const task = page.render({ canvasContext: pdfCtx, viewport, canvas: pdfCanvas });
      renderTaskRef.current = task;
      task.promise
        .then(() => redraw())
        .catch(() => {
          // Expected when cancel() above fires mid-render — the next
          // renderPage() call's own .then already redraws for the size
          // that superseded this one.
        });
    }
    redraw();
  }

  // Loads the plate whenever `url` changes — via ff-api's proxy (see the
  // module doc comment above for why this can't fetch aeronav.faa.gov
  // directly).
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    pageRef.current = null;
    const proxiedUrl = `${API_BASE_URL}/dtpp/plate?url=${encodeURIComponent(url)}`;
    const loadingTask = pdfjsLib.getDocument({ url: proxiedUrl });
    loadingTask.promise
      .then((pdf) => pdf.getPage(1))
      .then((page) => {
        if (cancelled) return;
        pageRef.current = page;
        setLoading(false);
        renderPage();
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });
    return () => {
      cancelled = true;
      void loadingTask.destroy();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [url]);

  // Re-fits on container resize — covers both a window resize and the
  // Full Page toggle (which changes the container's own CSS size, not
  // the window's).
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const observer = new ResizeObserver(() => renderPage());
    observer.observe(container);
    return () => observer.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expanded]);

  // Wheel-to-zoom, full-page only. A native (non-passive) listener rather
  // than React's onWheel so preventDefault actually takes — otherwise the
  // wheel would scroll the overlay instead of zooming. Inline mode leaves
  // the wheel alone (it's fit-only, and the page should scroll normally).
  useEffect(() => {
    const container = containerRef.current;
    if (!container || !expanded) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const factor = e.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP;
      setZoom((z) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z * factor)));
    };
    container.addEventListener("wheel", onWheel, { passive: false });
    return () => container.removeEventListener("wheel", onWheel);
  }, [expanded]);

  const zoomIn = () => setZoom((z) => Math.min(MAX_ZOOM, z * ZOOM_STEP));
  const zoomOut = () => setZoom((z) => Math.max(MIN_ZOOM, z / ZOOM_STEP));

  function commitDraftStroke() {
    const points = draftPointsRef.current;
    const color = activeColorRef.current;
    if (points.length >= 2 && color) {
      const { width, height } = cssSizeRef.current;
      const stroke: HighlighterStroke = {
        id: nextIdRef.current++,
        color,
        points: points.map(([x, y]): [number, number] => [x / width, y / height]),
      };
      setStrokes((prev) => [...prev, stroke]);
    }
    draftPointsRef.current = [];
    redraw();
  }

  function eraseAt(x: number, y: number) {
    const { width, height } = cssSizeRef.current;
    setStrokes((prev) =>
      prev.filter((s) => {
        const pxPoints = s.points.map(([fx, fy]): [number, number] => [fx * width, fy * height]);
        return !hitTestStroke(pxPoints, x, y, ERASER_HIT_RADIUS_PX);
      }),
    );
  }

  function handlePointerDown(e: React.PointerEvent<HTMLCanvasElement>) {
    const canvas = drawCanvasRef.current;
    if (!canvas) return;
    canvas.setPointerCapture(e.pointerId);
    if (eraseActiveRef.current) {
      isErasingRef.current = true;
      eraseAt(e.nativeEvent.offsetX, e.nativeEvent.offsetY);
      return;
    }
    if (!activeColorRef.current) return;
    isDrawingRef.current = true;
    draftPointsRef.current = [[e.nativeEvent.offsetX, e.nativeEvent.offsetY]];
    redraw();
  }
  function handlePointerMove(e: React.PointerEvent<HTMLCanvasElement>) {
    const x = e.nativeEvent.offsetX;
    const y = e.nativeEvent.offsetY;
    if (isErasingRef.current) {
      eraseAt(x, y);
      return;
    }
    if (!isDrawingRef.current) return;
    const points = draftPointsRef.current;
    const [lastX, lastY] = points[points.length - 1];
    if (distance(x, y, lastX, lastY) < SAMPLE_MIN_PX) return;
    points.push([x, y]);
    redraw();
  }
  function handlePointerUp() {
    if (isErasingRef.current) {
      isErasingRef.current = false;
      return;
    }
    if (!isDrawingRef.current) return;
    isDrawingRef.current = false;
    commitDraftStroke();
  }

  const toggleColor = (color: string) => setActiveColor((prev) => (prev === color ? null : color));
  const toggleErase = () => setEraseActive((prev) => !prev);
  const undo = () => setStrokes((prev) => prev.slice(0, -1));
  const clearAll = () => setStrokes([]);

  return (
    <>
      <div className={expanded ? "dtpp-chart-toolbar dtpp-chart-toolbar-expanded" : "dtpp-chart-toolbar"}>
        <div className="highlighter-toolbar">
          {HIGHLIGHTER_COLORS.map((c) => (
            <button
              key={c.id}
              className={activeColor === c.color ? "highlighter-swatch active" : "highlighter-swatch"}
              style={{ background: c.color }}
              aria-label={`Highlight in ${c.id}`}
              aria-pressed={activeColor === c.color}
              onClick={() => toggleColor(c.color)}
            />
          ))}
          <button className={eraseActive ? "selected" : ""} onClick={toggleErase}>
            Erase
          </button>
          <button onClick={undo} disabled={strokes.length === 0}>
            Undo
          </button>
          <button onClick={clearAll} disabled={strokes.length === 0}>
            Clear
          </button>
        </div>
        <div className="plate-toolbar-right">
          {/* Zoom is a full-page-only affordance — the inline plate is
              always fit-to-box (§9.1). */}
          {expanded && (
            <div className="plate-zoom-controls">
              <button onClick={zoomOut} disabled={zoom <= MIN_ZOOM} aria-label="Zoom out">
                −
              </button>
              <span className="plate-zoom-level">{Math.round(zoom * 100)}%</span>
              <button onClick={zoomIn} disabled={zoom >= MAX_ZOOM} aria-label="Zoom in">
                +
              </button>
            </div>
          )}
          <button onClick={onToggleExpanded}>{expanded ? (collapseLabel ?? "Reduce") : "Full Page"}</button>
        </div>
      </div>
      <div
        ref={containerRef}
        className={expanded ? "plate-canvas-wrap plate-canvas-wrap-expanded" : "plate-canvas-wrap"}
        role="img"
        aria-label={title}
      >
        <div ref={pageWrapRef} className="plate-page">
          <canvas ref={pdfCanvasRef} className="plate-pdf-canvas" />
          <canvas
            ref={drawCanvasRef}
            className="plate-draw-canvas"
            style={{ cursor: eraseActive ? "cell" : activeColor ? "crosshair" : "default" }}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerUp}
          />
        </div>
        {loading && <p className="hint plate-status">loading {title}…</p>}
        {error && <p className="hint plate-status">Failed to load plate: {error}</p>}
      </div>
    </>
  );
}
