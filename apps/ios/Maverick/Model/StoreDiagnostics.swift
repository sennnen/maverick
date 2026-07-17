import Foundation
import SQLite3

/// Path surface the diagnostics + data-protection screens read.
enum StorePaths {
  static func defaultDatabasePath() throws -> String { MavStore.databaseURL().path }
}

/// Read-only `PRAGMA quick_check` probe over the core's SQLite store (the Maverick check, re-based
/// from GRDB onto the raw C API — the probed file is never mutated).
enum DatabaseIntegrity {
  /// nil when healthy, else SQLite's first complaint (or the open/query error) verbatim.
  static func quickCheckFailure(atPath path: String) -> String? {
    guard FileManager.default.fileExists(atPath: path) else {
      return "no database file at that path"
    }
    var db: OpaquePointer?
    guard sqlite3_open_v2(path, &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK else {
      let message = db.map { String(cString: sqlite3_errmsg($0)) } ?? "cannot open database"
      sqlite3_close(db)
      return message
    }
    defer { sqlite3_close(db) }
    var statement: OpaquePointer?
    guard sqlite3_prepare_v2(db, "PRAGMA quick_check(1)", -1, &statement, nil) == SQLITE_OK else {
      return String(cString: sqlite3_errmsg(db))
    }
    defer { sqlite3_finalize(statement) }
    var rows: [String] = []
    while sqlite3_step(statement) == SQLITE_ROW {
      if let text = sqlite3_column_text(statement, 0) { rows.append(String(cString: text)) }
    }
    return Self.verdict(fromRows: rows)
  }

  /// nil = healthy (the single canonical "ok" row); otherwise the first complaint verbatim.
  /// An empty result set is a failure too — quick_check always answers.
  static func verdict(fromRows rows: [String]) -> String? {
    if rows.count == 1, rows[0].lowercased() == "ok" { return nil }
    return rows.first { $0.lowercased() != "ok" } ?? "quick_check returned no verdict"
  }
}
