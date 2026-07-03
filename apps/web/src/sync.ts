import { API_BASE_URL } from "./weather";

// Mirrors ff-sync's CycleManifest (crates/ff-sync/src/manifest.rs) —
// ff-api's /cycles/latest constructs that exact Rust type before
// serializing, so this shape can't silently drift from what's actually
// served the way an earlier, never-exercised version of the Rust type
// did (see manifest.rs's doc comment).
export interface CycleManifest {
  cycle_id: string;
  sqlite_url: string;
  sqlite_sha256: string;
  pmtiles_url: string | null;
  pmtiles_sha256: string | null;
}

interface CachedCycle {
  cycle_id: string;
  sqlite_sha256: string;
  bytes: ArrayBuffer;
  synced_at: number;
}

const DB_NAME = "freeflight-cycles";
const STORE_NAME = "cycles";

function openCacheDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => {
      req.result.createObjectStore(STORE_NAME, { keyPath: "cycle_id" });
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error as unknown);
  });
}

async function getCachedCycle(cycleId: string): Promise<CachedCycle | null> {
  const db = await openCacheDb();
  return new Promise((resolve, reject) => {
    const req = db.transaction(STORE_NAME, "readonly").objectStore(STORE_NAME).get(cycleId);
    req.onsuccess = () => resolve((req.result as CachedCycle | undefined) ?? null);
    req.onerror = () => reject(req.error as unknown);
  });
}

/** Whatever cycle was most recently cached, regardless of id — used when
 * ff-api can't be reached at all, so a prior sync still works offline. */
async function getMostRecentCachedCycle(): Promise<CachedCycle | null> {
  const db = await openCacheDb();
  return new Promise((resolve, reject) => {
    const req = db.transaction(STORE_NAME, "readonly").objectStore(STORE_NAME).getAll();
    req.onsuccess = () => {
      const all = (req.result as CachedCycle[] | undefined) ?? [];
      resolve(all.length === 0 ? null : all.reduce((a, b) => (a.synced_at >= b.synced_at ? a : b)));
    };
    req.onerror = () => reject(req.error as unknown);
  });
}

async function putCachedCycle(cycle: CachedCycle): Promise<void> {
  const db = await openCacheDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    tx.objectStore(STORE_NAME).put(cycle);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error as unknown);
  });
}

async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

async function fetchBundledDemoBytes(): Promise<Uint8Array> {
  const res = await fetch("/demo-cycle.sqlite");
  if (!res.ok) {
    throw new Error(`failed to fetch demo-cycle.sqlite: ${res.status}`);
  }
  return new Uint8Array(await res.arrayBuffer());
}

export type SyncSource = "synced" | "cached" | "bundled";

export interface SyncResult {
  bytes: Uint8Array;
  cycleId: string | null;
  source: SyncSource;
}

/**
 * Checks ff-api for the latest published cycle (DESIGN.md §8): reuses
 * the IndexedDB-cached copy if it's already the same cycle with a
 * matching checksum, otherwise downloads it, verifies the checksum
 * client-side (Web Crypto `SHA-256`, matching `ff-sync::verify_checksum`
 * on the Rust side), and caches it for next time.
 *
 * Falls back to whatever's cached (even if stale) when ff-api can't be
 * reached or a download/verify fails, and to the static bundled demo
 * data as a last resort — same "rest of the app stays usable" principle
 * as the weather panel and map overlays elsewhere in this app.
 *
 * Not the sqlite-wasm+OPFS storage DESIGN.md specifies — this keeps
 * sql.js and caches the raw bytes in IndexedDB instead, which covers
 * "persists across reloads" and "verify checksum before use" without
 * the bigger migration off sql.js (see apps/web/README.md).
 */
export async function syncCycle(): Promise<SyncResult> {
  let manifest: CycleManifest;
  try {
    const res = await fetch(`${API_BASE_URL}/cycles/latest`);
    if (!res.ok) throw new Error(`GET /cycles/latest failed: ${res.status}`);
    manifest = (await res.json()) as CycleManifest;
  } catch (err) {
    console.warn("couldn't reach ff-api to check for a newer cycle", err);
    const cached = await getMostRecentCachedCycle().catch(() => null);
    if (cached) {
      return { bytes: new Uint8Array(cached.bytes), cycleId: cached.cycle_id, source: "cached" };
    }
    return { bytes: await fetchBundledDemoBytes(), cycleId: null, source: "bundled" };
  }

  const cached = await getCachedCycle(manifest.cycle_id).catch(() => null);
  if (cached && cached.sqlite_sha256 === manifest.sqlite_sha256) {
    return { bytes: new Uint8Array(cached.bytes), cycleId: manifest.cycle_id, source: "synced" };
  }

  try {
    const res = await fetch(`${API_BASE_URL}${manifest.sqlite_url}`);
    if (!res.ok) throw new Error(`download failed: ${res.status}`);
    const bytes = await res.arrayBuffer();
    const actualSha256 = await sha256Hex(bytes);
    if (actualSha256 !== manifest.sqlite_sha256) {
      throw new Error(`checksum mismatch: expected ${manifest.sqlite_sha256}, got ${actualSha256}`);
    }
    await putCachedCycle({
      cycle_id: manifest.cycle_id,
      sqlite_sha256: manifest.sqlite_sha256,
      bytes,
      synced_at: Date.now(),
    });
    return { bytes: new Uint8Array(bytes), cycleId: manifest.cycle_id, source: "synced" };
  } catch (err) {
    console.warn("couldn't download/verify the latest cycle from ff-api", err);
    const fallback = cached ?? (await getMostRecentCachedCycle().catch(() => null));
    if (fallback) {
      return { bytes: new Uint8Array(fallback.bytes), cycleId: fallback.cycle_id, source: "cached" };
    }
    return { bytes: await fetchBundledDemoBytes(), cycleId: null, source: "bundled" };
  }
}
