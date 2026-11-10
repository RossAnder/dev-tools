Status: done — the After Merge bullet in the v2 pipeline idea doc now states the human-only handover contract.

Changed:
- docs/ideas/v2-pipeline.md: rewrote the After Merge bullet (label and position kept, two sentences). `[[handover]]` holds only human-only steps (design/handover.md#reasons), each with reason, action, when and done-when. The engine runs every agent-performable landing step itself and reports each undone handover in that shape and nothing else.

Deviations: none.
Handover: none.

Checks:
- Compile: not applicable (docs only).
- Accept `! grep -q 'an agent can be told to run' docs/ideas/v2-pipeline.md`: passes.

TANGENTIAL: none
