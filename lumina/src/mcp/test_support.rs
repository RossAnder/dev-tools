//! Shared test fixtures for the `mcp` module's per-family test modules.
//!
//! These helpers drive the tool handlers directly to seed prerequisite rows and
//! read structured payloads. They are `pub(crate)` so the sibling test modules
//! (`mcp::mod::tests`, `mcp::reads::tests`, `mcp::work_items::tests`) can share
//! them — a sibling module cannot see another sibling's private items, so the
//! cross-family fixtures live here rather than in any one family's file.

use super::*;

/// Build a legal project→epic→focus→story chain and return the story id so
/// the create-tool test can target a legal `task` parent.
pub(crate) async fn seed_chain_to_story(tools: &LuminaTools) -> String {
    // Migration-0010 valid chain: an epic must carry an outcome, a focus a
    // shape, and a story can only be created once its ancestor epic has ≥1
    // close-criterion — so the create-tool calls supply outcome/shape and the
    // seed adds the epic close-criterion via the `add_acceptance_criterion`
    // tool before the story create.
    async fn create(
        tools: &LuminaTools,
        kind: &str,
        parent: Option<&str>,
        outcome: Option<&str>,
        shape: Option<&str>,
    ) -> String {
        let res = tools
            .create_work_item(Parameters(CreateWorkItemRequest {
                kind: kind.to_owned(),
                parent_id: parent.map(str::to_owned),
                title: kind.to_uppercase(),
                body: None,
                origin: None,
                outcome: outcome.map(str::to_owned),
                shape: shape.map(str::to_owned),
            }))
            .await
            .expect("legal create");
        // The structured content carries `{ "id": "<uuid>" }`.
        let value = res.structured_content.expect("structured id payload");
        value["id"].as_str().expect("id string").to_owned()
    }

    let project = create(tools, "project", None, None, None).await;
    let epic = create(tools, "epic", Some(&project), Some("the epic outcome"), None).await;
    tools
        .add_acceptance_criterion(Parameters(AddAcceptanceCriterionParams {
            work_item_id: epic.clone(),
            text: "epic close criterion".to_owned(),
        }))
        .await
        .expect("epic close criterion");
    let focus =
        create(tools, "focus", Some(&epic), None, Some("vertical-slice")).await;
    create(tools, "story", Some(&focus), None, None).await
}

/// Create a work item via the tool handler and return its id.
pub(crate) async fn create_item(tools: &LuminaTools, kind: &str, parent: Option<&str>) -> String {
    let res = tools
        .create_work_item(Parameters(CreateWorkItemRequest {
            kind: kind.to_owned(),
            parent_id: parent.map(str::to_owned),
            title: format!("{kind} item"),
            body: None,
            origin: None,
            outcome: None,
            shape: None,
        }))
        .await
        .expect("legal create");
    res.structured_content
        .expect("structured id payload")["id"]
        .as_str()
        .expect("id string")
        .to_owned()
}

/// Read the `id` out of a write tool's structured payload.
pub(crate) fn id_of(res: &CallToolResult) -> String {
    res.structured_content
        .as_ref()
        .expect("structured payload")["id"]
        .as_str()
        .expect("id string")
        .to_owned()
}
