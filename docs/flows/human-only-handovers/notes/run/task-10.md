Status: done — plan-risk and plan-structure lenses inverted on post-landing steps

Changed:
- claude/skills/review-v2-lenses/lenses/plan-risk.md: back-compat bullet now names migration as a task, or an After Merge/handover step only when human-only; new probe bullet flagging agent-performable, bundled or reasonless/triggerless/doneless After Merge bullets and `[[handover]]` rows (fix points to design/handover.md#where-agent-performable-steps-go-instead); `add` edit now "a Risks line or a task"; `minor` calibration drops the missing-After-Merge-note clause and adds the new finding as minor, major when it is the only verification of the goal.
- claude/skills/review-v2-lenses/lenses/plan-structure.md: predicted-criterion bullet now requires a `success:` key or accept check (or a human-only attended/observe handover naming the criterion); After Merge removed from the `risk` advisory and the `critical` calibration.

Deviations: none.
Checks: both grep accept checks pass; `tomlctl review catalogue --builtin claude/skills/review-v2-lenses --check` returns ok. No compile check needed (markdown).

TANGENTIAL: none
