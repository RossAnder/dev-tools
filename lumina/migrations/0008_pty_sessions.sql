CREATE TABLE pty_sessions (
    id TEXT PRIMARY KEY,                  -- uuid v7
    label TEXT,                           -- user-set, nullable
    project_id TEXT REFERENCES work_items(id) ON DELETE SET NULL,
    cwd TEXT NOT NULL,
    config_json TEXT NOT NULL,            -- SpawnConfig snapshot
    parse_strategy_version INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL,                 -- spawning|active|idle|awaiting|completed|failed|cancelled
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    ended_at TEXT,                        -- nullable
    exit_code INTEGER,
    last_error TEXT,
    previous_session_id TEXT REFERENCES pty_sessions(id) ON DELETE SET NULL
);

CREATE INDEX idx_pty_sessions_project ON pty_sessions(project_id) WHERE project_id IS NOT NULL;
CREATE INDEX idx_pty_sessions_status ON pty_sessions(status);

-- Enforce that project_id, when set, references a row where kind='project'.
-- Mirrors the BEFORE INSERT trigger pattern from migrations 0004/0005 — a
-- column CHECK cannot subquery sibling rows in SQLite, so the guard runs as
-- a trigger. Same shape applies to UPDATE statements that mutate project_id.
CREATE TRIGGER pty_sessions_project_kind_check_insert
BEFORE INSERT ON pty_sessions
FOR EACH ROW WHEN NEW.project_id IS NOT NULL
BEGIN
  SELECT CASE
    WHEN (SELECT kind FROM work_items WHERE id = NEW.project_id) != 'project'
    THEN RAISE(ABORT, 'pty_sessions.project_id must reference a work_items row with kind=project')
  END;
END;

CREATE TRIGGER pty_sessions_project_kind_check_update
BEFORE UPDATE OF project_id ON pty_sessions
FOR EACH ROW WHEN NEW.project_id IS NOT NULL
BEGIN
  SELECT CASE
    WHEN (SELECT kind FROM work_items WHERE id = NEW.project_id) != 'project'
    THEN RAISE(ABORT, 'pty_sessions.project_id must reference a work_items row with kind=project')
  END;
END;

CREATE TABLE pty_messages (
    id TEXT PRIMARY KEY,                  -- uuid v7
    session_id TEXT NOT NULL REFERENCES pty_sessions(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,            -- per-session monotonic
    created_at TEXT NOT NULL,
    kind TEXT NOT NULL,                   -- user_input|assistant_text|tool_call|tool_result|prompt|system|error|parser_unknown
    content_json TEXT NOT NULL,           -- typed payload
    raw_text TEXT,                        -- ansi-stripped fallback, nullable
    UNIQUE(session_id, sequence)
);

CREATE INDEX idx_pty_messages_session ON pty_messages(session_id, sequence);

CREATE TABLE pty_queue (
    id TEXT PRIMARY KEY,                  -- uuid v7
    session_id TEXT NOT NULL REFERENCES pty_sessions(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    input_kind TEXT NOT NULL,             -- prompt|cancel|control
    payload TEXT NOT NULL,
    enqueued_at TEXT NOT NULL,
    dispatched_at TEXT,
    completed_at TEXT,
    status TEXT NOT NULL,                 -- pending|dispatched|completed|failed|cancelled
    error TEXT,
    UNIQUE(session_id, sequence)
);

CREATE INDEX idx_pty_queue_pending ON pty_queue(session_id, sequence) WHERE status = 'pending';
