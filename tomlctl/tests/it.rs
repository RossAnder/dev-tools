//! The integration tests, linked as one executable. Each sibling file is a
//! module here, so an edit to the library relinks one test binary instead of
//! one per file. `tasks_corpus` stays its own target because it is run by
//! name.

mod common;

mod agent_contract;
mod agents_audit;
mod agents_record;
mod array_predicates;
mod backlog_check_batch;
mod backlog_edit;
mod backlog_queue;
mod backlog_read;
mod backlog_reconcile;
mod backlog_write;
mod blocks;
mod broken_pipe;
mod capabilities;
mod clusters;
mod doc_writes;
mod field_flags;
mod flow_active;
mod flow_doctor;
mod flow_ensure_artifact;
mod flow_envelope;
mod flow_find_plans;
mod flow_init;
mod flow_list;
mod flow_path_seam;
mod flow_record;
mod flow_resolve;
mod flow_stale;
mod id_aliases;
mod id_minting;
mod in_process;
mod inputs;
mod integration;
mod integrity_guards;
mod items_dedupe;
mod items_dry_run;
mod items_get_ids;
mod items_id_prefix;
mod json;
mod last_updated_stamp;
mod library_facade;
mod list_grep;
mod list_limited;
mod output_nesting;
mod output_options;
mod output_trim;
mod payload_toml;
mod plan_approve;
mod plan_check;
mod plan_drift;
mod plan_emit;
mod plan_lifecycle;
mod plan_milestone;
mod plan_open;
mod plan_probes;
mod plan_records;
mod plan_refusals;
mod plan_run_records;
mod plan_stage;
mod plan_view;
mod render_progress_log;
mod review_add;
mod review_anchor;
mod review_apply;
mod review_catalogue;
mod review_cells;
mod review_cover;
mod review_digest;
mod review_export;
mod review_finish;
mod review_plan_records;
mod review_query;
mod review_reconcile;
mod review_reproduce;
mod review_resolve;
mod review_run_show;
mod review_runs;
mod review_schedule;
mod review_settle;
mod review_stats;
mod review_store_v2;
mod review_survey;
mod review_sync;
mod review_triage;
mod review_verdict;
mod review_vocabulary;
mod store_guard;
mod sweep;
mod tasks_audit;
mod tasks_finding_links;
mod tasks_graph;
mod tasks_import;
mod tasks_read;
mod tasks_render;
mod tasks_snapshot;
mod tasks_spec_add;
mod tasks_train;
mod tasks_write;
mod updated_stamp;

/// A sibling file missing from the list above compiles into no target, since
/// the manifest turns test auto-discovery off, and its tests would never run.
#[test]
fn every_test_file_is_a_module_of_this_binary() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let declared = include_str!("it.rs");
    let missing: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            let stem = path.file_stem()?.to_str()?.to_string();
            (path.extension()? == "rs").then_some(stem)
        })
        .filter(|stem| stem != "it" && stem != "tasks_corpus")
        .filter(|stem| !declared.contains(&format!("\nmod {stem};")))
        .collect();
    assert!(
        missing.is_empty(),
        "test files that are not modules of tests/it.rs: {missing:?}"
    );
}
