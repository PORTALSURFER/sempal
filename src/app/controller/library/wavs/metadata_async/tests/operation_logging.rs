use super::*;
use std::io;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    fn captured(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl<'a> MakeWriter<'a> for SharedBuffer {
    type Writer = SharedBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedBufferWriter(self.0.clone())
    }
}

struct SharedBufferWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for SharedBufferWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn capture_info_logs<F>(run: F) -> String
where
    F: FnOnce(),
{
    let buffer = SharedBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::INFO)
        .with_writer(buffer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    buffer.captured()
}

#[test]
fn source_metadata_job_logs_operation_path_remap_and_result() {
    let _lock = metadata_async_test_lock();
    let temp = tempfile::tempdir().expect("create temp dir");
    let source = SampleSource::new(temp.path().join("source"));
    std::fs::create_dir_all(&source.root).expect("create source root");
    let old_relative = PathBuf::from("old-name.wav");
    let new_relative = PathBuf::from("new-name.wav");
    let old_absolute = source.root.join(&old_relative);
    let new_absolute = source.root.join(&new_relative);
    std::fs::write(&old_absolute, b"metadata-fixture").expect("write fixture");

    let db =
        SourceDatabase::open_for_test_fixture_source_write(&source.root).expect("open source db");
    let (old_size, old_modified_ns) =
        crate::app::controller::library::wav_io::file_metadata(&old_absolute)
            .expect("old metadata");
    db.upsert_file(&old_relative, old_size, old_modified_ns)
        .expect("insert old row");
    std::fs::rename(&old_absolute, &new_absolute).expect("rename fixture");
    let (new_size, new_modified_ns) =
        crate::app::controller::library::wav_io::file_metadata(&new_absolute)
            .expect("new metadata");
    let mut batch = db.write_batch().expect("start rename batch");
    batch.remove_file(&old_relative).expect("remove old row");
    batch
        .upsert_file(&new_relative, new_size, new_modified_ns)
        .expect("insert new row");
    batch.commit().expect("commit rename batch");
    let _rename_scope = source_write_priority::CompletedBrowserRenameTestGuard::new(
        &source.id,
        &old_relative,
        &new_relative,
    );

    let captured = capture_info_logs(|| {
        let result = run_metadata_mutation_job(MetadataMutationJob {
            request_id: 17,
            source_id: source.id.clone(),
            source_root: source.root.clone(),
            paths: [old_relative.clone()].into_iter().collect(),
            source_ops: vec![SourceMetadataMutationOp::SetLooped {
                relative_path: old_relative.clone(),
                looped: true,
            }],
            analysis_ops: Vec::new(),
        });
        assert!(result.result.is_ok(), "{:?}", result.result);
    });

    assert!(
        captured.contains("source metadata mutation: source ops resolved"),
        "source metadata batch should log resolved operations: {captured}"
    );
    assert!(
        captured.contains("request_id=17")
            && captured.contains("op_count=1")
            && captured.contains("result=\"ok\"")
            && captured.contains("SetLooped old-name.wav -> new-name.wav remapped=true"),
        "log should include op name, original path, resolved path, and result: {captured}"
    );
}
