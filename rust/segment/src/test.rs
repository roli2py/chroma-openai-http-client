use std::sync::atomic::AtomicU32;

use chroma_blockstore::{provider::BlockfileProvider, test_arrow_blockfile_provider};
use chroma_index::{hnsw_provider::HnswIndexProvider, test_hnsw_index_provider};
use chroma_types::{
    test_segment, Chunk, Collection, CollectionAndSegments, CollectionUuid, LogRecord, Segment,
    SegmentScope,
};

use super::{
    blockfile_metadata::MetadataSegmentWriter, blockfile_record::RecordSegmentWriter,
    distributed_hnsw::DistributedHNSWSegmentWriter, types::materialize_logs,
};

#[derive(Clone)]
pub struct TestSegment {
    pub blockfile_provider: BlockfileProvider,
    pub hnsw_provider: HnswIndexProvider,
    pub collection: Collection,
    pub metadata_segment: Segment,
    pub record_segment: Segment,
    pub vector_segment: Segment,
}

impl TestSegment {
    pub fn new_with_dimension(dimension: usize) -> Self {
        let collection_uuid = CollectionUuid::new();
        let collection = Collection {
            collection_id: collection_uuid,
            name: "Test Collection".to_string(),
            metadata: None,
            dimension: Some(dimension as i32),
            tenant: "Test Tenant".to_string(),
            database: String::new(),
            log_position: 0,
            version: 0,
            total_records_post_compaction: 0,
        };
        Self {
            blockfile_provider: test_arrow_blockfile_provider(2 << 22),
            hnsw_provider: test_hnsw_index_provider(),
            collection,
            metadata_segment: test_segment(collection_uuid, SegmentScope::METADATA),
            record_segment: test_segment(collection_uuid, SegmentScope::RECORD),
            vector_segment: test_segment(collection_uuid, SegmentScope::VECTOR),
        }
    }

    // WARN: The size of the log chunk should not be too large
    pub async fn compact_log(&mut self, logs: Chunk<LogRecord>, next_offset: usize) {
        let materialized_logs =
            materialize_logs(&None, logs, Some(AtomicU32::new(next_offset as u32).into()))
                .await
                .expect("Should be able to materialize log.");

        let mut metadata_writer =
            MetadataSegmentWriter::from_segment(&self.metadata_segment, &self.blockfile_provider)
                .await
                .expect("Should be able to initialize metadata writer.");
        metadata_writer
            .apply_materialized_log_chunk(&None, &materialized_logs)
            .await
            .expect("Should be able to apply materialized logs.");
        metadata_writer
            .finish()
            .await
            .expect("Should be able to write to blockfile.");
        self.metadata_segment.file_path = metadata_writer
            .commit()
            .await
            .expect("Should be able to commit metadata.")
            .flush()
            .await
            .expect("Should be able to flush metadata.");

        let record_writer =
            RecordSegmentWriter::from_segment(&self.record_segment, &self.blockfile_provider)
                .await
                .expect("Should be able to initiaize record writer.");
        record_writer
            .apply_materialized_log_chunk(&None, &materialized_logs)
            .await
            .expect("Should be able to apply materialized log.");

        self.record_segment.file_path = record_writer
            .commit()
            .await
            .expect("Should be able to commit record.")
            .flush()
            .await
            .expect("Should be able to flush record.");

        let vector_writer = DistributedHNSWSegmentWriter::from_segment(
            &self.vector_segment,
            self.collection
                .dimension
                .expect("Collection dimension should be set") as usize,
            self.hnsw_provider.clone(),
        )
        .await
        .expect("Should be able to initialize vector writer");

        vector_writer
            .apply_materialized_log_chunk(&None, &materialized_logs)
            .await
            .expect("Should be able to apply materialized log.");

        self.vector_segment.file_path = vector_writer
            .commit()
            .await
            .expect("Should be able to commit vector.")
            .flush()
            .await
            .expect("Should be able to flush vector.");
    }
}

impl From<TestSegment> for CollectionAndSegments {
    fn from(value: TestSegment) -> Self {
        Self {
            collection: value.collection,
            metadata_segment: value.metadata_segment,
            record_segment: value.record_segment,
            vector_segment: value.vector_segment,
        }
    }
}

impl Default for TestSegment {
    fn default() -> Self {
        Self::new_with_dimension(128)
    }
}
