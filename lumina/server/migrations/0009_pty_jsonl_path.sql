-- Migration 0009: add jsonl_path to pty_sessions
--
-- jsonl_path: nullable TEXT column that binds the per-session JSONL transcript
-- file written by `claude` interactive mode at
-- ~/.claude/projects/<sanitised-cwd>/<uuid>.jsonl. Populated by the JSONL-tail
-- bridge after spawn; NULL until that binding occurs.
--
-- pty_messages.kind vocabulary (forward, replacing the 0008 SSOT comment at
-- line 49): user_input | assistant_text | tool_use | tool_result | system | error
-- (drops tool_call / prompt / parser_unknown — those were vt100-parser artefacts).

ALTER TABLE pty_sessions ADD COLUMN jsonl_path TEXT;
