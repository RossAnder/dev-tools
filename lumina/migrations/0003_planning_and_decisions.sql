-- lumina migration 0003: planning- and decision-grade schema (Plan 1.5).
--
-- Additive / non-destructive over 0001 + 0002. Three classes of change:
--   1. New nullable columns on work_items (relevance, effort, complexity,
--      origin, closure_gate, blocked_by_question_id, enabling_option_id),
--      findings (origin, confidence, superseded_by self-FK), and
--      work_item_activity (origin).
--   2. Four new FK child tables (acceptance_criteria, research_notes,
--      open_questions, question_options) mirroring the 0002 work_item_activity
--      idiom: TEXT PK, parent FK ON DELETE CASCADE, monotonic `seq`,
--      `created_at` default, UNIQUE(parent, seq), and a (parent, seq) index.
--   3. (no new triggers — no new JSON columns ⇒ no validity guard needed.)
--
-- Design constraints (mirroring 0001 / 0002):
--   * SQL kept ANSI-ish for a later Postgres port. All `id` columns are TEXT
--     holding an app-generated UUIDv7 (generation lives in the repo layer /
--     tests; the schema only declares TEXT). The new columns are plain nullable
--     TEXT — typed enums (Relevance/Effort/Complexity/Origin/…) are validated in
--     the repo layer, free TEXT in the DB (mirroring how 0001/Plan 1 typed
--     status/severity).
--   * SQLite ALTER TABLE ADD COLUMN cannot carry a non-constant DEFAULT, a
--     PK/UNIQUE, or a retroactive CHECK. An `ADD COLUMN ... REFERENCES` is legal
--     ONLY with its implicit NULL default — adding a non-NULL DEFAULT to such a
--     column aborts under foreign_keys=ON. The self-FK columns
--     (findings.superseded_by, plus research_notes.superseded_by which is part of
--     a fresh CREATE TABLE) therefore carry NO DEFAULT.
--   * ADD COLUMN leaves existing rows at NULL (= "unset") for every new column —
--     intended and non-destructive.
--   * The PRAGMA below is for consistency only; FK enforcement (and thus the
--     ON DELETE CASCADE behaviour these child tables rely on) is actually
--     established per-connection via SqliteConnectOptions::foreign_keys(true) in
--     src/db.rs, NOT by this statement.

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- work_items: additive planning/decision columns (all nullable TEXT).
--   relevance             — active|backlog|deferred|rejected (epic/focus/story
--                           scope; task/project left NULL); validated in repo.
--   effort                — s|m|l (batch sizing); validated in repo.
--   complexity            — low|medium|high (model tier); validated in repo.
--   origin                — plan|implement|review|optimise|tdd|human|none
--                           provenance; validated in repo.
--   closure_gate          — hard|soft (story scope); gates task->done when hard.
--   blocked_by_question_id — task is blocked while this open_question is open.
--   enabling_option_id    — task is exclusive to this question_option's branch.
-- ---------------------------------------------------------------------------
ALTER TABLE work_items ADD COLUMN relevance TEXT;
ALTER TABLE work_items ADD COLUMN effort TEXT;
ALTER TABLE work_items ADD COLUMN complexity TEXT;
ALTER TABLE work_items ADD COLUMN origin TEXT;
ALTER TABLE work_items ADD COLUMN closure_gate TEXT;
ALTER TABLE work_items ADD COLUMN blocked_by_question_id TEXT;
ALTER TABLE work_items ADD COLUMN enabling_option_id TEXT;

-- ---------------------------------------------------------------------------
-- findings: weighting / provenance / supersession.
--   origin        — same provenance enum as work_items.origin (free TEXT here).
--   confidence    — high|medium|low evidence grade (validated in repo).
--   superseded_by — self-FK to the finding that supersedes this one; live
--                   findings are `superseded_by IS NULL`. NO DEFAULT (implicit
--                   NULL) — an ADD COLUMN ... REFERENCES with a non-NULL DEFAULT
--                   would abort under foreign_keys=ON.
-- ---------------------------------------------------------------------------
ALTER TABLE findings ADD COLUMN origin TEXT;
ALTER TABLE findings ADD COLUMN confidence TEXT;
ALTER TABLE findings ADD COLUMN superseded_by TEXT REFERENCES findings(id);

-- ---------------------------------------------------------------------------
-- work_item_activity: provenance stamp so record_task_activity can mark origin.
-- ---------------------------------------------------------------------------
ALTER TABLE work_item_activity ADD COLUMN origin TEXT;

-- ---------------------------------------------------------------------------
-- acceptance_criteria: per-task checkable criteria (the work_item_activity
-- idiom). `seq` is a per-work_item monotonic ordinal (repo allocates);
-- UNIQUE(work_item_id, seq) makes gaps/dupes structurally impossible.
-- ON DELETE CASCADE ties criteria to the owning item's lifetime.
--   checked      — 0/1 flag (free INTEGER; repo flips it).
--   checked_at   — ISO-8601 TEXT timestamp of the check (NULL = unchecked).
--   checked_by   — actor who checked (NULL = unchecked / unattributed).
-- ---------------------------------------------------------------------------
CREATE TABLE acceptance_criteria (
    id           TEXT PRIMARY KEY,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    text         TEXT NOT NULL,
    checked      INTEGER NOT NULL DEFAULT 0,
    checked_at   TEXT,
    checked_by   TEXT,
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (work_item_id, seq)
);

CREATE INDEX idx_acceptance_criteria_work_item ON acceptance_criteria(work_item_id, seq);

-- ---------------------------------------------------------------------------
-- research_notes: first-class research records with confidence + accept/reject
-- + supersession (the work_item_activity idiom + a self-FK supersession chain).
--   confidence    — high|medium|low evidence grade (validated in repo).
--   state         — proposed|accepted|rejected lifecycle (validated in repo).
--   rationale     — free-text accept/reject justification.
--   lens          — the analytical lens the note was produced under.
--   origin        — provenance enum (free TEXT; validated in repo).
--   superseded_by — self-FK to the note that supersedes this one; live notes are
--                   `superseded_by IS NULL`. Part of this CREATE TABLE (not an
--                   ALTER), so a REFERENCES with NULL default is fine.
-- ON DELETE CASCADE ties notes to the owning item's lifetime.
-- ---------------------------------------------------------------------------
CREATE TABLE research_notes (
    id            TEXT PRIMARY KEY,
    work_item_id  TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    seq           INTEGER NOT NULL,
    summary       TEXT NOT NULL,
    body          TEXT,
    confidence    TEXT,
    state         TEXT,
    rationale     TEXT,
    lens          TEXT,
    origin        TEXT,
    superseded_by TEXT REFERENCES research_notes(id),
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (work_item_id, seq)
);

CREATE INDEX idx_research_notes_work_item ON research_notes(work_item_id, seq);

-- ---------------------------------------------------------------------------
-- open_questions: story-scoped decision lifecycle (the work_item_activity
-- idiom). `seq` is a per-story monotonic ordinal; UNIQUE(story_id, seq) holds.
-- ON DELETE CASCADE ties questions to the owning story's lifetime.
--   status              — open|answered|cancelled lifecycle (validated in repo).
--   answer              — free-text resolution narrative (NULL while open).
--   chosen_option_id    — the question_option picked on resolution (NULL while
--                         open); NOT a hard FK (resolution sets it after the
--                         option exists; left soft to avoid an ordering bind).
--   decided_at/_by      — resolution audit (NULL while open).
--   prompting_finding_id — the finding that surfaced this question (back-link).
--   prompting_note_id    — the research_note that surfaced this question.
-- ---------------------------------------------------------------------------
CREATE TABLE open_questions (
    id                   TEXT PRIMARY KEY,
    story_id             TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    seq                  INTEGER NOT NULL,
    question             TEXT NOT NULL,
    status               TEXT,
    answer               TEXT,
    chosen_option_id     TEXT,
    decided_at           TEXT,
    decided_by           TEXT,
    prompting_finding_id TEXT REFERENCES findings(id),
    prompting_note_id    TEXT REFERENCES research_notes(id),
    created_at           TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (story_id, seq)
);

CREATE INDEX idx_open_questions_story ON open_questions(story_id, seq);

-- ---------------------------------------------------------------------------
-- question_options: the answer-option branches of an open_question (the
-- work_item_activity idiom). `seq` is per-question; UNIQUE(question_id, seq).
-- ON DELETE CASCADE ties options to the owning question's lifetime.
--   label  — short option label.
--   detail — optional longer description.
-- ---------------------------------------------------------------------------
CREATE TABLE question_options (
    id          TEXT PRIMARY KEY,
    question_id TEXT NOT NULL REFERENCES open_questions(id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,
    label       TEXT NOT NULL,
    detail      TEXT,
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (question_id, seq)
);

CREATE INDEX idx_question_options_question ON question_options(question_id, seq);
