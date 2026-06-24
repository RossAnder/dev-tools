-- lumina migration 0027: covering index on task_research_links(research_note_id)
-- so a research_notes delete does not scan the link table (review finding R7).
--
-- ## Why
-- 0026's task_research_links carries a composite PRIMARY KEY
-- (task_id, research_note_id). SQLite indexes a composite PK by its LEAD
-- column, so task_id is covered (list_task_research_links and the task-side
-- ON DELETE CASCADE both use it), but research_note_id has NO supporting
-- index. A hard-delete of a research_notes row must therefore SCAN
-- task_research_links to honour the note-side ON DELETE CASCADE. Notes are
-- normally SUPERSEDED (an UPDATE, no cascade), so the scan is latent today;
-- this index turns the note-side cascade into an index lookup if note
-- hard-deletes ever become common.
--
-- A NEW forward migration, NOT an edit to the applied 0026: editing a
-- committed migration breaks its sqlx checksum and forces a dev-DB recreate.
-- Forward-only; no down-migration. LF line endings (a CRLF migration risks an
-- sqlx checksum anomaly on a future renormalize -- see 0001's known CRLF issue).

CREATE INDEX idx_task_research_links_note
    ON task_research_links(research_note_id);
