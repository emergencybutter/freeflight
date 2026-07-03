import initSqlJs, { type Database, type SqlValue } from "sql.js";
// Vite's `?url` import gives back a hashed, build-safe URL instead of
// requiring a manual copy-to-public step for the wasm binary.
import sqlWasmUrl from "sql.js/dist/sql-wasm.wasm?url";
import { syncCycle, type SyncSource } from "./sync";

export interface LoadedDatabase {
  db: Database;
  cycleId: string | null;
  source: SyncSource;
}

/**
 * Loads the active cycle bundle with sql.js: synced from `ff-api` if
 * reachable (checksum-verified, cached in IndexedDB for next time —
 * see sync.ts), falling back to a previously-cached cycle or the static
 * bundled demo data otherwise.
 */
export async function loadDatabase(): Promise<LoadedDatabase> {
  const SQL = await initSqlJs({ locateFile: () => sqlWasmUrl });
  const { bytes, cycleId, source } = await syncCycle();
  return { db: new SQL.Database(bytes), cycleId, source };
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
