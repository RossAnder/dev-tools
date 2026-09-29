//! `tomlctl agents list` — the records of one flow's agents store, in store
//! order.

use std::path::Path;

use anyhow::Result;
use serde_json::Value as JsonValue;

use super::schema;
use crate::cli::{ReadIntegrityArgs, read_integrity_opts};
use crate::convert::toml_to_json;
use crate::io;

/// The store's `agents` array as JSON. A missing store lists `[]` — no hook
/// has fired for the flow yet — unless `--strict-read` asks for
/// `kind=not_found`. The rows pass through the schema, so a malformed or
/// newer-versioned store errors here instead of being echoed as-is.
pub(crate) fn rows(path: &Path, read_opts: &ReadIntegrityArgs) -> Result<JsonValue> {
    io::strict_read_check(path, read_opts.strict_read)?;
    if !path.exists() {
        return Ok(JsonValue::Array(Vec::new()));
    }
    io::read_doc(path, read_integrity_opts(read_opts), |doc| {
        let store = schema::from_toml(doc)?;
        let agents = schema::to_toml(&store)
            .get("agents")
            .map(toml_to_json)
            .unwrap_or_else(|| JsonValue::Array(Vec::new()));
        Ok(agents)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::schema::{AgentRecord, AgentsStore};

    fn read_args(strict_read: bool) -> ReadIntegrityArgs {
        ReadIntegrityArgs {
            verify_integrity: false,
            strict_read,
        }
    }

    fn ids(rows: &JsonValue) -> Vec<&str> {
        rows.as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn an_absent_store_lists_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(schema::STORE_FILE);
        assert_eq!(
            rows(&path, &read_args(false)).unwrap(),
            JsonValue::Array(Vec::new())
        );
    }

    #[test]
    fn an_absent_store_is_not_found_under_strict_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(schema::STORE_FILE);
        let err = rows(&path, &read_args(true)).unwrap_err();
        assert_eq!(
            err.downcast_ref::<crate::errors::TaggedError>()
                .map_or("other", |tagged| tagged.kind.as_str()),
            "not_found"
        );
    }

    #[test]
    fn a_seeded_store_lists_its_rows_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(schema::STORE_FILE);
        let record = |id: &str, agent_type: &str| AgentRecord {
            id: id.into(),
            session_id: "s1".into(),
            agent_id: format!("agent-{id}"),
            agent_type: agent_type.into(),
            ..AgentRecord::default()
        };
        // Deliberately not in id order: the listing follows the store.
        let store = AgentsStore {
            agents: vec![
                record("A2", "implement-deep"),
                record("A1", "research-lite"),
                record("A3", "verification"),
            ],
            ..AgentsStore::default()
        };
        let text = toml::to_string(&schema::to_toml(&store)).unwrap();
        std::fs::write(&path, text).unwrap();

        let listed = rows(&path, &read_args(false)).unwrap();
        assert_eq!(ids(&listed), ["A2", "A1", "A3"]);
        assert_eq!(listed[0]["agent_type"], "implement-deep");
        assert_eq!(listed[1]["status"], "running");
        assert_eq!(listed[2]["agent_id"], "agent-A3");
    }
}
