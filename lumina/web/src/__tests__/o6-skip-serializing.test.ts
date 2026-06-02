// O6 (2026-06-02) wire-contract regression guard.
//
// The Rust `domain::WorkItem` / `domain::Finding` Option fields now carry
// `#[serde(skip_serializing_if = "Option::is_none")]`, so an unset field is
// ABSENT from the JSON rather than emitted as `null`. The zod schemas were
// migrated from a bare `.nullable()` to `.nullable().default(null)` so an absent
// key is normalised back to `null` on parse — keeping the parsed type `T | null`
// and every `x === null` consumer (e.g. the `parent_id === null` root-detection
// in treeUtils / useHierarchy) working unchanged.
//
// These tests parse the POST-O6 wire shape (nullable keys OMITTED) and assert
// (a) the parse succeeds and (b) the omitted fields come back as `null`, not
// `undefined`. If a future change reverts a field to a bare `.nullable()`, the
// parse of the omitted-key object below fails and this guard fires.

import { describe, expect, test } from 'bun:test'

import { WorkItemSchema, FindingSchema } from '../api/work-items'

describe('O6 skip_serializing_if tolerance', () => {
  test('WorkItemSchema parses a root with every nullable key OMITTED → null', () => {
    // Only the non-nullable required fields are present (mirrors a root project
    // row whose parent_id / body / position / attributes / … are all unset and
    // therefore omitted by serde).
    const wire = {
      id: 'wi-1',
      kind: 'project',
      title: 'P',
      status: 'open',
      created_at: '2026-06-02T00:00:00Z',
      updated_at: '2026-06-02T00:00:00Z',
    }
    const parsed = WorkItemSchema.safeParse(wire)
    expect(parsed.success).toBe(true)
    if (parsed.success) {
      // Critical for root detection: an absent parent_id must normalise to null
      // (NOT undefined) so `parent_id === null` keeps identifying roots.
      expect(parsed.data.parent_id).toBeNull()
      expect(parsed.data.body).toBeNull()
      expect(parsed.data.position).toBeNull()
      expect(parsed.data.attributes).toBeNull()
      expect(parsed.data.relevance).toBeNull()
      expect(parsed.data.task_kind).toBeNull()
      expect(parsed.data.tier).toBeNull()
      expect(parsed.data.shape).toBeNull()
    }
  })

  test('WorkItemSchema still accepts explicit null and concrete values', () => {
    const parsed = WorkItemSchema.safeParse({
      id: 'wi-2',
      kind: 'task',
      parent_id: 's-1',
      title: 'T',
      body: null,
      status: 'todo',
      position: null,
      attributes: null,
      relevance: null,
      effort: 'm',
      complexity: null,
      origin: null,
      closure_gate: null,
      blocked_by_question_id: null,
      enabling_option_id: null,
      task_kind: null,
      tier: 'deep',
      created_at: '2026-06-02T00:00:00Z',
      updated_at: '2026-06-02T00:00:00Z',
    })
    expect(parsed.success).toBe(true)
    if (parsed.success) {
      expect(parsed.data.parent_id).toBe('s-1')
      expect(parsed.data.effort).toBe('m')
      expect(parsed.data.tier).toBe('deep')
      expect(parsed.data.body).toBeNull()
    }
  })

  test('FindingSchema parses a live finding with every nullable key OMITTED → null', () => {
    // Only the always-populated required fields are present (kind / severity /
    // category / status / summary are set on every live finding, so O6's
    // skip_serializing_if never fires on them).
    const wire = {
      id: 'f1',
      kind: 'bug',
      severity: 'major',
      category: 'review',
      status: 'open',
      summary: 'a finding',
    }
    const parsed = FindingSchema.safeParse(wire)
    expect(parsed.success).toBe(true)
    if (parsed.success) {
      expect(parsed.data.work_item_id).toBeNull()
      expect(parsed.data.effort).toBeNull()
      expect(parsed.data.line).toBeNull()
      expect(parsed.data.origin).toBeNull()
      expect(parsed.data.resolution).toBeNull()
      expect(parsed.data.wontfix_rationale).toBeNull()
    }
  })
})
