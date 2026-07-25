use super::*;

#[test]
fn reconcile_reports_and_retains_malformed_journal_rows() {
    let temp = TempDir::new().unwrap();
    let target_root = temp.path().join("target");
    std::fs::create_dir_all(&target_root).unwrap();
    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    target_db
        .connection
        .execute(
            "INSERT INTO file_ops_journal (
                 id, op_type, stage, source_root, source_relative, target_relative,
                 staged_relative, file_size, modified_ns, tag, looped, locked, last_played_at,
                 created_at
               )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                "bad-row",
                "move",
                "intent",
                Option::<String>::None,
                Option::<String>::None,
                "/absolute.wav",
                Option::<String>::None,
                Option::<i64>::None,
                Option::<i64>::None,
                Option::<i64>::None,
                Option::<i64>::None,
                Option::<i64>::None,
                Option::<i64>::None,
                1i64,
            ],
        )
        .unwrap();

    let summary = reconcile_pending_ops_for_tests(&target_db).unwrap();
    assert_eq!(summary.total, 1);
    assert_eq!(summary.completed, 0);
    assert_eq!(summary.errors.len(), 1);
    assert!(summary.errors[0].contains("bad-row"));
    assert!(summary.errors[0].contains("retained journal row"));
    let entry_count = target_db
        .connection
        .query_row(
            "SELECT COUNT(*) FROM file_ops_journal",
            [],
            |row: &rusqlite::Row<'_>| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(entry_count, 1);
}
