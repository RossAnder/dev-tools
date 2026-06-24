// Bun tests for the floating-chat slice.
//
// Two layers:
//   1. T2 pure resolvers (`floatingChatContext.ts`): `resolveFocalPoint` from a
//      fake focusPath (incl. the field/row path) and `resolveCwd` precedence
//      (local_path → clone_root → null). No state, no seam.
//   2. T3 composable (`useFloatingChat.ts`) via `__setApiForTests` /
//      `__setSettingsForTests`: open-flow spawn(cwd)→submitBatch payload
//      (context + first-prompt atomic), ≥2 canned templates → exact
//      context-bearing prompt, freeform submit frame, close()→deleteSession,
//      spawn-error + empty-focus states, and popup-session independence from the
//      [03] console singleton.
//
// Mirrors the seam idiom in `pty-session.test.ts` / `readiness.test.ts`:
// `__setApiForTests` swaps an in-memory adapter, `__resetForTests` clears
// module-singleton state between tests. Bun has no DOM/SFC compiler, so this is
// composable-level only (the FloatingChat.vue card is covered by type-check/build
// in T4).

import { beforeEach, describe, expect, test } from 'bun:test'

import type {
  InputFrame,
  PtySession,
  SessionStream,
  WsFrame,
  WsFrameType,
} from '../api/pty'
import type { WorkItem, WorkItemDetail, RepoLink } from '../api'

import {
  resolveFocalPoint,
  resolveCwd,
  type FieldDescriptor,
} from '../composables/floatingChatContext'
import {
  useFloatingChat,
  cannedTemplates,
  __resetForTests,
  __setApiForTests,
  __setSettingsForTests,
  __setSessionApiForTests,
} from '../composables/useFloatingChat'
import {
  usePtySession,
  __resetForTests as __resetConsoleSession,
  __setApiForTests as __setConsoleApi,
} from '../composables/usePtySession'

// ---------------------------------------------------------------------------
// Fixtures + helpers.
// ---------------------------------------------------------------------------

function makeWorkItem(id: string, overrides: Partial<WorkItem> = {}): WorkItem {
  return {
    id,
    kind: 'story',
    parent_id: null,
    title: id,
    body: null,
    status: 'open',
    position: 0,
    attributes: null,
    relevance: null,
    effort: null,
    complexity: null,
    origin: null,
    closure_gate: null,
    blocked_by_question_id: null,
    enabling_option_id: null,
    task_kind: null,
    tier: null,
    shape: null,
    plan_epoch: 0,
    created_at: '2026-06-12T00:00:00Z',
    updated_at: '2026-06-12T00:00:00Z',
    ...overrides,
  }
}

function makeSession(id: string, overrides: Partial<PtySession> = {}): PtySession {
  return {
    id,
    label: null,
    project_id: null,
    cwd: '/tmp',
    config_json: '{}',
    parse_strategy_version: 1,
    status: 'awaiting',
    started_at: '2026-06-12T12:00:00Z',
    updated_at: '2026-06-12T12:00:00Z',
    ended_at: null,
    exit_code: null,
    last_error: null,
    previous_session_id: null,
    ...overrides,
  }
}

function makeRepoLink(overrides: Partial<RepoLink> = {}): RepoLink {
  return {
    id: 'rl-1',
    project_id: 'p-1',
    slug: 'owner/name',
    position: 0,
    is_primary: 1,
    created_at: '2026-06-12T00:00:00Z',
    local_path: null,
    ...overrides,
  }
}

function makeProjectDetail(repoLinks: RepoLink[]): WorkItemDetail {
  return {
    item: makeWorkItem('p-1', { kind: 'project' }),
    children: [],
    findings: [],
    context_blocks: [],
    activity: [],
    acceptance_criteria: [],
    research_notes: [],
    open_questions: [],
    repo_links: repoLinks,
    risks: [],
    rejected_alternatives: [],
    task_dependencies: [],
    story_files_footprint: [],
    task_research_links: [],
  }
}

/** Controllable `SessionStream` mock with an `emit` hook + observable sends. */
interface MockStream extends SessionStream {
  emit: (frame: WsFrame) => void
  sent: InputFrame[]
  closed: boolean
}

function makeMockStream(): MockStream {
  const handlers = new Map<WsFrameType, Array<(frame: WsFrame) => void>>()
  const sent: InputFrame[] = []
  const handle: MockStream = {
    send(frame) {
      sent.push(frame)
    },
    on(event, handler) {
      const list = handlers.get(event)
      if (list) list.push(handler)
      else handlers.set(event, [handler])
    },
    close() {
      handle.closed = true
    },
    emit(frame) {
      const list = handlers.get(frame.type)
      if (list) for (const fn of list) fn(frame)
    },
    sent,
    closed: false,
  }
  return handle
}

// ===========================================================================
// 1. T2 pure resolvers.
// ===========================================================================

describe('resolveFocalPoint', () => {
  test('item-scope: builds the snapshot from focusPath + focusedNode (no field)', () => {
    const project = makeWorkItem('p-1', { kind: 'project' })
    const story = makeWorkItem('s-1', { kind: 'story' })
    const focusPath = [project, story]

    const fp = resolveFocalPoint(focusPath, story)

    expect(fp.workItemId).toBe('s-1')
    expect(fp.kind).toBe('story')
    // Root-first, exactly as useHierarchy.focusPath returns it.
    expect(fp.ancestryPath).toEqual([project, story])
    expect(fp.fieldKey).toBeUndefined()
    expect(fp.field).toBeUndefined()
    expect(fp.nestedRowId).toBeUndefined()
  })

  test('field-scope: maps descriptor.field → fieldKey and descriptor.rowId → nestedRowId', () => {
    const story = makeWorkItem('s-1', { kind: 'story' })
    const focusPath = [story]
    const descriptor: FieldDescriptor = {
      workItemId: 's-1',
      kind: 'story',
      field: 'problem_statement',
      collection: 'research_notes',
      rowId: 'note-7',
    }

    const fp = resolveFocalPoint(focusPath, story, descriptor)

    expect(fp.workItemId).toBe('s-1')
    expect(fp.fieldKey).toBe('problem_statement')
    expect(fp.field).toBe('problem_statement')
    expect(fp.collection).toBe('research_notes')
    // descriptor.rowId rides into nestedRowId — the row path (T2 FieldDescriptor).
    expect(fp.nestedRowId).toBe('note-7')
  })
})

describe('resolveCwd precedence', () => {
  test('prefers the project primary repo_links.local_path', () => {
    const detail = makeProjectDetail([
      makeRepoLink({ is_primary: 0, local_path: '/secondary/clone' }),
      makeRepoLink({ id: 'rl-2', is_primary: 1, local_path: '/primary/clone' }),
    ])
    expect(resolveCwd(detail, { cloneRoot: '/machine/root' })).toBe('/primary/clone')
  })

  test('falls back to clone_root when no primary local_path', () => {
    const detail = makeProjectDetail([
      makeRepoLink({ is_primary: 1, local_path: null }),
    ])
    expect(resolveCwd(detail, { cloneRoot: '/machine/root' })).toBe('/machine/root')
  })

  test('returns null when neither a primary local_path nor a clone_root is set', () => {
    const detail = makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: null })])
    expect(resolveCwd(detail, { cloneRoot: null })).toBeNull()
  })

  test('null project detail with a clone_root yields the clone_root', () => {
    expect(resolveCwd(null, { cloneRoot: '/machine/root' })).toBe('/machine/root')
  })
})

// ===========================================================================
// 2. T3 composable (useFloatingChat).
// ===========================================================================

describe('useFloatingChat', () => {
  beforeEach(() => {
    __resetForTests()
  })

  // A focal point whose project ancestor has a primary clone path so cwd
  // resolution succeeds without a settings clone_root.
  function focalPointWithClone(): ReturnType<typeof resolveFocalPoint> {
    const project = makeWorkItem('p-1', { kind: 'project' })
    const story = makeWorkItem('s-1', { kind: 'story' })
    return resolveFocalPoint([project, story], story)
  }

  test('open: spawns with the resolved cwd then submitBatch injects context + first prompt atomically', async () => {
    const fp = focalPointWithClone()
    const spawned = makeSession('pop-1', { status: 'idle' })
    const stream = makeMockStream()

    let spawnedCwd: string | null = null
    let batchedTo: string | null = null
    let batched: Array<{ kind: string; payload: string }> | null = null

    const chat = useFloatingChat()

    // Project-detail provides the primary clone path → cwd.
    __setApiForTests({
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: '/clone/here' })]),
      spawnSession: async (req) => {
        spawnedCwd = req.cwd
        return spawned
      },
    })
    // The popup session's own network seam (select → getSession/getMessages/WS,
    // submitBatch). getSession returns an `awaiting` row so waitForAwaiting
    // short-circuits with no real delay.
    setSessionSeam({
      getSession: async () => spawned,
      getMessages: async () => [],
      openSessionStream: () => stream,
      sendInputsBatch: async (id, frames) => {
        batchedTo = id
        batched = frames
      },
    })

    const result = await chat.open(fp, 'summarise')

    expect(result.ok).toBe(true)
    expect(spawnedCwd).toBe('/clone/here')

    // Atomic: ONE submitBatch carrying [context, firstPrompt] — context first.
    expect(batchedTo).toBe('pop-1')
    expect(batched).not.toBeNull()
    expect(batched).toHaveLength(2)
    expect(batched![0]!.kind).toBe('prompt')
    // Context frame names the focal point + the MCP-only constraint.
    expect(batched![0]!.payload).toContain('work item s-1')
    expect(batched![0]!.payload).toMatch(/mcp__lumina__/)
    expect(batched![0]!.payload).toMatch(/never call the HTTP API/i)
    // First prompt is the summarise canned op, also context-bearing + MCP-only.
    expect(batched![1]!.payload).toBe(cannedTemplates.summarise.build(fp))
    expect(batched![1]!.payload).toContain('work item s-1')

    expect(chat.isOpen.value).toBe(true)
    expect(chat.focalPoint.value?.workItemId).toBe('s-1')
    expect(chat.error.value).toBeNull()
  })

  test('canned templates: ≥2 ops each build an exact context-bearing, MCP-only prompt', () => {
    const fp = focalPointWithClone()

    const keys = Object.keys(cannedTemplates)
    expect(keys.length).toBeGreaterThanOrEqual(2)

    for (const key of keys) {
      const prompt = cannedTemplates[key as keyof typeof cannedTemplates].build(fp)
      // Context-bearing: names the focal work item.
      expect(prompt).toContain('work item s-1')
      // MCP-only: references the lumina MCP surface.
      expect(prompt).toMatch(/mcp__lumina__|lumina MCP tools/)
    }

    // Exact-prompt assertion for the two write-shaped ops.
    expect(cannedTemplates['next-action'].build(fp)).toMatch(/Never mutate via HTTP/)
    expect(cannedTemplates['critique-field'].build(fp)).toMatch(/only via mcp__lumina__|via the lumina MCP tools/)
  })

  test('canned op with a field-scoped focal point names the field', () => {
    const story = makeWorkItem('s-1', { kind: 'story' })
    const fp = resolveFocalPoint([story], story, {
      workItemId: 's-1',
      kind: 'story',
      field: 'problem_statement',
    })
    const prompt = cannedTemplates['critique-field'].build(fp)
    expect(prompt).toContain('"problem_statement"')
  })

  test('runCannedOp submits a single context-bearing prompt frame against the live session', async () => {
    const fp = focalPointWithClone()
    const spawned = makeSession('pop-1', { status: 'idle' })
    const stream = makeMockStream()

    const chat = useFloatingChat()
    __setApiForTests({
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: '/clone/here' })]),
      spawnSession: async () => spawned,
    })
    setSessionSeam({
      getSession: async () => spawned,
      getMessages: async () => [],
      openSessionStream: () => stream,
      sendInputsBatch: async () => {},
    })

    await chat.open(fp, 'summarise')
    const result = await chat.runCannedOp('next-action')

    expect(result.ok).toBe(true)
    // submit() goes over the WS stream as a single prompt input frame.
    expect(stream.sent).toHaveLength(1)
    expect(stream.sent[0]).toEqual({
      type: 'input',
      kind: 'prompt',
      payload: cannedTemplates['next-action'].build(fp),
    })
  })

  test('sendFreeform submits the operator text verbatim as one prompt frame', async () => {
    const fp = focalPointWithClone()
    const spawned = makeSession('pop-1', { status: 'idle' })
    const stream = makeMockStream()

    const chat = useFloatingChat()
    __setApiForTests({
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: '/clone/here' })]),
      spawnSession: async () => spawned,
    })
    setSessionSeam({
      getSession: async () => spawned,
      getMessages: async () => [],
      openSessionStream: () => stream,
      sendInputsBatch: async () => {},
    })

    await chat.open(fp, 'summarise')
    const result = await chat.sendFreeform('what is blocking this?')

    expect(result.ok).toBe(true)
    expect(stream.sent).toHaveLength(1)
    expect(stream.sent[0]).toEqual({
      type: 'input',
      kind: 'prompt',
      payload: 'what is blocking this?',
    })
  })

  test('close DELETEs the transient session and resets module state', async () => {
    const fp = focalPointWithClone()
    const spawned = makeSession('pop-1', { status: 'idle' })
    const stream = makeMockStream()

    let deletedId: string | null = null
    const chat = useFloatingChat()
    __setApiForTests({
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: '/clone/here' })]),
      spawnSession: async () => spawned,
      deleteSession: async (id) => {
        deletedId = id
      },
    })
    setSessionSeam({
      getSession: async () => spawned,
      getMessages: async () => [],
      openSessionStream: () => stream,
      sendInputsBatch: async () => {},
    })

    await chat.open(fp, 'summarise')
    expect(chat.sessionId.value).toBe('pop-1')

    const result = await chat.close()

    expect(result.ok).toBe(true)
    expect(deletedId).toBe('pop-1')
    // State reset.
    expect(chat.isOpen.value).toBe(false)
    expect(chat.focalPoint.value).toBeNull()
    expect(chat.sessionId.value).toBeNull()
    // The live WS was torn down.
    expect(stream.closed).toBe(true)
  })

  test('spawn error: no session, error state, isOpen stays true for the banner', async () => {
    const fp = focalPointWithClone()
    const chat = useFloatingChat()
    __setApiForTests({
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: '/clone/here' })]),
      spawnSession: async () => {
        throw new Error('spawn failed: conpty boom')
      },
    })

    const result = await chat.open(fp, 'summarise')

    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error).toMatch(/conpty boom/)
    expect(chat.sessionId.value).toBeNull()
    expect(chat.error.value).toMatch(/conpty boom/)
    // Open captured the focal point → the UI shows the header + the error.
    expect(chat.isOpen.value).toBe(true)
    expect(chat.focalPoint.value?.workItemId).toBe('s-1')
  })

  test('empty focus (workItemId falsy): no spawn, "select an item" error state', async () => {
    const chat = useFloatingChat()
    let spawnCalled = false
    __setApiForTests({
      spawnSession: async () => {
        spawnCalled = true
        return makeSession('nope')
      },
    })

    // A focal point with an empty workItemId (focusId === null upstream).
    const emptyFp = {
      workItemId: '',
      kind: 'story',
      ancestryPath: [],
    } as ReturnType<typeof resolveFocalPoint>

    const result = await chat.open(emptyFp)

    expect(result.ok).toBe(false)
    expect(spawnCalled).toBe(false)
    expect(chat.sessionId.value).toBeNull()
    expect(chat.error.value).toMatch(/select a work item/i)
  })

  test('null cwd (no clone path): no spawn, error state', async () => {
    const fp = focalPointWithClone()
    const chat = useFloatingChat()
    let spawnCalled = false
    __setApiForTests({
      // Primary link has no local_path → falls through to clone_root.
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: null })]),
      spawnSession: async () => {
        spawnCalled = true
        return makeSession('nope')
      },
    })
    // …and the machine clone_root is unset → resolveCwd returns null.
    __setSettingsForTests(() => ({ cloneRoot: null }))

    const result = await chat.open(fp)

    expect(result.ok).toBe(false)
    expect(spawnCalled).toBe(false)
    expect(chat.error.value).toMatch(/no clone path/i)
  })

  test('non-fatal project-fetch error does not leak into a successful open() banner', async () => {
    const fp = focalPointWithClone()
    const spawned = makeSession('pop-1', { status: 'idle' })
    const stream = makeMockStream()

    const chat = useFloatingChat()
    __setApiForTests({
      // The project fetch fails — but it is NON-FATAL: cwd resolves via the
      // machine clone_root fallback below, so open() still succeeds.
      fetchProjectDetail: async () => {
        throw new Error('project fetch boom')
      },
      spawnSession: async () => spawned,
    })
    __setSettingsForTests(() => ({ cloneRoot: '/machine/root' }))
    setSessionSeam({
      getSession: async () => spawned,
      getMessages: async () => [],
      openSessionStream: () => stream,
      sendInputsBatch: async () => {},
    })

    const result = await chat.open(fp, 'summarise')

    // Success despite the failed (non-fatal) fetch — and NO stale banner.
    expect(result.ok).toBe(true)
    expect(chat.error.value).toBeNull()
  })

  test('a prior open() error is cleared by a subsequent successful open()', async () => {
    const fp = focalPointWithClone()
    const chat = useFloatingChat()

    // First open(): spawn throws → error banner set, ok=false.
    __setApiForTests({
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: '/clone/here' })]),
      spawnSession: async () => {
        throw new Error('spawn failed: conpty boom')
      },
    })
    const first = await chat.open(fp, 'summarise')
    expect(first.ok).toBe(false)
    expect(chat.error.value).toMatch(/conpty boom/)

    // Second open(): everything succeeds → the prior banner is cleared.
    const spawned = makeSession('pop-1', { status: 'idle' })
    const stream = makeMockStream()
    __setApiForTests({
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: '/clone/here' })]),
      spawnSession: async () => spawned,
    })
    setSessionSeam({
      getSession: async () => spawned,
      getMessages: async () => [],
      openSessionStream: () => stream,
      sendInputsBatch: async () => {},
    })
    const second = await chat.open(fp, 'summarise')
    expect(second.ok).toBe(true)
    expect(chat.error.value).toBeNull()
  })

  test('settings clone_root is used when the project has no primary local_path', async () => {
    const fp = focalPointWithClone()
    const spawned = makeSession('pop-1', { status: 'idle' })
    const stream = makeMockStream()

    let spawnedCwd: string | null = null
    const chat = useFloatingChat()
    __setApiForTests({
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: null })]),
      spawnSession: async (req) => {
        spawnedCwd = req.cwd
        return spawned
      },
    })
    __setSettingsForTests(() => ({ cloneRoot: '/machine/clone/root' }))
    setSessionSeam({
      getSession: async () => spawned,
      getMessages: async () => [],
      openSessionStream: () => stream,
      sendInputsBatch: async () => {},
    })

    const result = await chat.open(fp)
    expect(result.ok).toBe(true)
    expect(spawnedCwd).toBe('/machine/clone/root')
  })

  // -------------------------------------------------------------------------
  // Popup-session independence from the [03] console singleton (T1 factory).
  // -------------------------------------------------------------------------

  test('popup session is independent of the [03] console singleton', async () => {
    // Drive the [03] console singleton to focus its own session.
    __resetConsoleSession()
    const consoleStream = makeMockStream()
    __setConsoleApi({
      getSession: async () => makeSession('console-1', { status: 'active' }),
      getMessages: async () => [],
      openSessionStream: () => consoleStream,
    })
    const console03 = usePtySession()
    await console03.select('console-1')
    expect(console03.currentId.value).toBe('console-1')

    // Now open the popup against a different session.
    const fp = focalPointWithClone()
    const popupSpawned = makeSession('pop-1', { status: 'idle' })
    const popupStream = makeMockStream()
    const chat = useFloatingChat()
    __setApiForTests({
      fetchProjectDetail: async () =>
        makeProjectDetail([makeRepoLink({ is_primary: 1, local_path: '/clone/here' })]),
      spawnSession: async () => popupSpawned,
    })
    setSessionSeam({
      getSession: async () => popupSpawned,
      getMessages: async () => [],
      openSessionStream: () => popupStream,
      sendInputsBatch: async () => {},
    })

    await chat.open(fp, 'summarise')

    // The popup focused pop-1; the [03] console is UNTOUCHED.
    expect(chat.session.currentId.value).toBe('pop-1')
    expect(console03.currentId.value).toBe('console-1')
    // Distinct ref objects — not a shared singleton.
    expect(chat.session.currentId).not.toBe(console03.currentId)
  })
})

// ---------------------------------------------------------------------------
// Popup-session network seam helper.
//
// The popup session is an INDEPENDENT makePtySessionComposable() instance inside
// useFloatingChat, so its api adapter is NOT the same object as the composable's
// own Api seam (spawn/delete/project-fetch). `__setSessionApiForTests` reaches
// the popup session's OWN seam (getSession/getMessages/openSessionStream/
// sendInputsBatch) so the open flow's select+submitBatch path is fully stubbed.
// ---------------------------------------------------------------------------

function setSessionSeam(override: Parameters<typeof __setSessionApiForTests>[0]): void {
  __setSessionApiForTests(override)
}
