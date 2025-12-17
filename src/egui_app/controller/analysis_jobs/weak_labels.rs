use rusqlite::{Connection, TransactionBehavior, params};

#[derive(Debug, Clone)]
pub(super) struct WeakLabelInsert {
    pub(super) class_id: String,
    pub(super) confidence: f32,
    pub(super) rule_id: String,
}

#[derive(Debug, Clone)]
pub(super) struct SampleWeakLabels {
    pub(super) sample_id: String,
    pub(super) labels: Vec<WeakLabelInsert>,
}

pub(super) fn replace_weak_labels_for_samples(
    conn: &mut Connection,
    samples: &[SampleWeakLabels],
    ruleset_version: i64,
    created_at: i64,
) -> Result<usize, String> {
    if samples.is_empty() {
        return Ok(0);
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start weak label transaction: {err}"))?;
    let mut inserted = 0usize;
    {
        let mut delete_stmt = tx
            .prepare("DELETE FROM labels_weak WHERE sample_id = ?1")
            .map_err(|err| format!("Failed to prepare weak label delete: {err}"))?;
        let mut insert_stmt = tx
            .prepare(
                "INSERT INTO labels_weak (sample_id, ruleset_version, class_id, confidence, rule_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|err| format!("Failed to prepare weak label insert: {err}"))?;

        for sample in samples {
            delete_stmt.execute(params![sample.sample_id]).map_err(|err| {
                format!("Failed to delete weak labels for {}: {err}", sample.sample_id)
            })?;
            for label in &sample.labels {
                let changed = insert_stmt
                    .execute(params![
                        sample.sample_id,
                        ruleset_version,
                        label.class_id,
                        label.confidence,
                        label.rule_id,
                        created_at
                    ])
                    .map_err(|err| {
                        format!(
                            "Failed to insert weak label for {} ({}): {err}",
                            sample.sample_id, label.class_id
                        )
                    })?;
                inserted += changed;
            }
        }
    }

    tx.commit()
        .map_err(|err| format!("Failed to commit weak label transaction: {err}"))?;
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn_with_schema() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE labels_weak (
                sample_id TEXT NOT NULL,
                ruleset_version INTEGER NOT NULL,
                class_id TEXT NOT NULL,
                confidence REAL NOT NULL,
                rule_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (sample_id, class_id)
            ) WITHOUT ROWID;",
        )
        .unwrap();
        conn
    }

    #[test]
    fn replace_deletes_and_inserts() {
        let mut conn = conn_with_schema();
        conn.execute(
            "INSERT INTO labels_weak (sample_id, ruleset_version, class_id, confidence, rule_id, created_at)
             VALUES ('s1', 1, 'kick', 0.5, 'old', 1)",
            [],
        )
        .unwrap();

        let samples = vec![SampleWeakLabels {
            sample_id: "s1".to_string(),
            labels: vec![WeakLabelInsert {
                class_id: "snare".to_string(),
                confidence: 0.9,
                rule_id: "drums.snare.filename".to_string(),
            }],
        }];
        let inserted = replace_weak_labels_for_samples(&mut conn, &samples, 1, 10).unwrap();
        assert_eq!(inserted, 1);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM labels_weak WHERE sample_id = 's1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);

        let (class_id, rule_id): (String, String) = conn
            .query_row(
                "SELECT class_id, rule_id FROM labels_weak WHERE sample_id = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(class_id, "snare");
        assert_eq!(rule_id, "drums.snare.filename");
    }
}
