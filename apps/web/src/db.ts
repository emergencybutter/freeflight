import initSqlJs, { type Database, type SqlValue } from "sql.js";
// Vite's `?url` import gives back a hashed, build-safe URL instead of
// requiring a manual copy-to-public step for the wasm binary.
import sqlWasmUrl from "sql.js/dist/sql-wasm.wasm?url";

/**
 * Loads the demo cycle bundle (DESIGN.md §7/§8 — a stand-in for the real
 * `cycle-*.sqlite` bundle `ff-etl` will eventually publish) with sql.js.
 * This is read-only and in-memory, so there's no OPFS/persistence story
 * here yet; that's needed once the client downloads its own cycles.
 */
export async function loadDemoDatabase(): Promise<Database> {
  const SQL = await initSqlJs({ locateFile: () => sqlWasmUrl });
  const response = await fetch("/demo-cycle.sqlite");
  if (!response.ok) {
    throw new Error(`failed to fetch demo-cycle.sqlite: ${response.status}`);
  }
  const buffer = await response.arrayBuffer();
  return new SQL.Database(new Uint8Array(buffer));
}

/** Runs a query and maps result rows to plain objects keyed by column name. */
export function queryAll<T>(db: Database, sql: string, params: SqlValue[] = []): T[] {
  const stmt = db.prepare(sql);
  stmt.bind(params);
  const rows: T[] = [];
  while (stmt.step()) {
    rows.push(stmt.getAsObject() as T);
  }
  stmt.free();
  return rows;
}
