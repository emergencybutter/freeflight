// HTML fragments describing what's under a tapped map point, one
// function per tap tab (Airspace/AIRMET/SIGMET/CWA/PIREPs) — used by
// App.tsx's tab panels via dangerouslySetInnerHTML. Used to live inline
// in MapView.tsx as MapLibre Popup content; moved here once tapping
// switched from per-layer Popups to a single unified tap handler whose
// results get displayed in tabs below the map instead.

/** Escapes text from third-party APIs (aviationweather.gov, this app's
 * own airspace/fix data) before it goes into `dangerouslySetInnerHTML`
 * — treat it like any other untrusted string rather than assuming it's
 * safe to interpolate raw. */
export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function airspaceInfoHtml(props: Record<string, unknown>): string {
  const { name, class: cls, floor, ceiling } = props as {
    name: string;
    class: string;
    floor: string;
    ceiling: string;
  };
  const label = cls === "B" || cls === "C" || cls === "D" ? `Class ${cls}` : cls;
  return `<strong>${escapeHtml(label)}: ${escapeHtml(name)}</strong><br>${escapeHtml(floor)}–${escapeHtml(ceiling)}`;
}

/** No raw bulletin text like SIGMET/PIREP have, so this is built from
 * the structured fields instead. FZLVL (freezing level) reports
 * fzlbase/fzltop rather than base/top (confirmed live: FZLVL records
 * have null base/top). */
export function gairmetInfoHtml(props: Record<string, unknown>): string {
  const hazard = props.hazard as string;
  const severity = props.severity as string | null;
  const base = props.base as string | null;
  const top = props.top as string | null;
  const fzlbase = props.fzlbase as string | null;
  const fzltop = props.fzltop as string | null;
  const validTime = props.validTime as string;
  const altitude = hazard === "FZLVL" ? [fzlbase, fzltop].filter(Boolean).join("–") : [base, top].filter(Boolean).join("–");
  const parts = [`<strong>G-AIRMET: ${escapeHtml(hazard)}${severity ? ` (${escapeHtml(severity)})` : ""}</strong>`];
  if (altitude) parts.push(escapeHtml(altitude));
  parts.push(`<span style="opacity: 0.7;">valid ${escapeHtml(validTime)}</span>`);
  return parts.join("<br>");
}

export function sigmetInfoHtml(props: Record<string, unknown>): string {
  const hazard = props.hazard as string;
  const low = props.altitudeLow1 as number | null;
  const hi = props.altitudeHi1 as number | null;
  const raw = props.rawAirSigmet as string;
  const altitude = low !== null || hi !== null ? `${low ?? "SFC"}–${hi ?? "unlimited"} ft` : null;
  const parts = [`<strong>SIGMET: ${escapeHtml(hazard)}</strong>`];
  if (altitude) parts.push(escapeHtml(altitude));
  parts.push(`<span style="font-family: monospace; font-size: 0.85em;">${escapeHtml(raw)}</span>`);
  return parts.join("<br>");
}

export function cwaInfoHtml(props: Record<string, unknown>): string {
  const hazard = props.hazard as string;
  const cwsu = props.cwsu as string;
  const rawText = props.rawText as string;
  return [
    `<strong>CWA: ${escapeHtml(hazard)} (${escapeHtml(cwsu)} Center)</strong>`,
    `<span style="font-family: monospace; font-size: 0.85em; white-space: pre-line;">${escapeHtml(rawText)}</span>`,
  ].join("<br>");
}

export function pirepInfoHtml(props: Record<string, unknown>): string {
  const { summary, rawOb } = props as { summary: string; rawOb: string };
  return `<strong>${escapeHtml(summary) || "PIREP"}</strong><br><span style="font-family: monospace; font-size: 0.85em;">${escapeHtml(rawOb)}</span>`;
}
