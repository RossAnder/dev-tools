Status: done — plan target tests now use the handover action key

- tomlctl/src/plan/target.rs: test target `handover:H1.command` became `handover:H1.action`, and the PLAN fixture's `[[handover]]` row now carries `action` in place of `command`. No non-test code named the key.
- Deviation: the edit was made with `sed -i` rather than Edit (two single-line substitutions; the diff shows exactly 2 changed lines, so line endings were unaffected).
- Checks: the grep accept check passes. The cargo test accept check writes the build, so it is left to the gate. No compile check was run (test-only string edits).

TANGENTIAL: none
