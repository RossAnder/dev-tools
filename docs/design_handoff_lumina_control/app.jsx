/* LUMINA — main app */
/* global React, ReactDOM */

const { useState, useEffect, useMemo, useRef, useCallback } = React;
const D = window.LUMINA_DATA;

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "showWatermark": false,
  "watermarkOpacity": 0.04,
  "watermarkSize": 220
}/*EDITMODE-END*/;

/* ============================================================
   Helpers
   ============================================================ */

const TYPE_LABEL = {
  root: "ROOT",
  epic: "EPIC",
  feature: "FEATURE",
  story: "STORY",
  task: "TASK",
};

function pctText(n) {
  return Math.round((n || 0) * 100) + "%";
}

// Story-size T-shirt mapping. Backed by the same point numbers in the data
// so other rollups keep working, but the UI shows S/M/L/XL instead.
function sizeOf(pts) {
  if (!pts) return "—";
  if (pts <= 2) return "S";
  if (pts <= 5) return "M";
  if (pts <= 8) return "L";
  return "XL";
}

function sizeDistribution(nodes) {
  const counts = { S: 0, M: 0, L: 0, XL: 0 };
  for (const n of nodes) {
    if (!n || !n.points) continue;
    counts[sizeOf(n.points)]++;
  }
  return ["S", "M", "L", "XL"]
    .filter((k) => counts[k] > 0)
    .map((k) => `${counts[k]}${k}`)
    .join(" · ") || "—";
}

function getPathTo(id) {
  if (!id) return [];
  const path = [];
  let cur = D.byId[id];
  while (cur) {
    path.unshift(cur);
    cur = cur.parent ? D.byId[cur.parent] : null;
  }
  return path;
}

function getChildren(id) {
  if (!id) return D.epics;
  const node = D.byId[id];
  if (!node || !node.children) return [];
  return node.children.map((cid) => D.byId[cid]).filter(Boolean);
}

function getSiblings(id) {
  if (!id) return [];
  const node = D.byId[id];
  if (!node || !node.parent) {
    // top-level: siblings are other epics
    return D.epics.filter((e) => e.id !== id);
  }
  const parent = D.byId[node.parent];
  return parent.children
    .map((cid) => D.byId[cid])
    .filter((n) => n && n.id !== id);
}

/* ============================================================
   Status pill
   ============================================================ */

function Status({ status }) {
  const cls = "status " + (status || "");
  const label = (status || "draft").replace("-", " ");
  return (
    <span className={cls}>
      <span className="sdot"></span>
      {label}
    </span>
  );
}

function SectionLabel({ num, children, sep }) {
  return (
    <div className="sec-label">
      {num && <span className="num">{num}</span>}
      {num && <span className="sep">/</span>}
      <span>{children}</span>
      {sep && <span className="sep">·</span>}
      {sep && <span>{sep}</span>}
    </div>
  );
}

/* ============================================================
   Header
   ============================================================ */

function Header({ sprint, activeAgents }) {
  return (
    <header className="header">
      <div className="brand">
        <div className="brand-mark"><span>L</span></div>
        <div>
          <div className="brand-name"><b>LUMINA</b> / CONTROL</div>
          <div className="brand-version">v0.3.1-α · noir</div>
        </div>
      </div>

      <div className="header-center">
        <div className="cmd-bar">
          <span style={{ color: "var(--faint)" }}>›</span>
          <input placeholder="JUMP TO EPIC / FEATURE / STORY / TASK · OR /COMMAND" />
          <span className="kbd">⌘K</span>
        </div>
      </div>

      <div className="header-right">
        <div className="pill">
          <span className="dot live"></span>
          <span>SPRINT&nbsp;&nbsp;{sprint.id}</span>
          <span style={{ color: "var(--ghost)" }}>·</span>
          <span style={{ color: "var(--ink-2)" }}>{sprint.queue.length}/{sprint.capacity} ITEMS</span>
        </div>
        <div className="pill">
          <span className="dot ok"></span>
          <span>{activeAgents} AGENT{activeAgents === 1 ? "" : "S"} · LIVE</span>
        </div>
        <div className="pill" style={{ borderColor: "transparent", background: "transparent" }}>
          <span style={{ color: "var(--faint)" }}>22 MAY 2026</span>
        </div>
      </div>
    </header>
  );
}

/* ============================================================
   Left rail — Hierarchy Spine
   ============================================================ */

function HierarchySpine({ focusId, onFocus }) {
  const path = useMemo(() => getPathTo(focusId), [focusId]);
  const siblingsOfFocus = useMemo(() => getSiblings(focusId), [focusId]);

  return (
    <div className="col left">
      <SectionLabel num="01">PLANNING GRAPH</SectionLabel>

      {/* If no focus, show epic list */}
      {!focusId && (
        <div className="epic-list">
          {D.epics.map((e) => (
            <div key={e.id} className="epic-row" onClick={() => onFocus(e.id)}>
              <div>
                <div className="spine-type">EPIC · {e.id}</div>
                <div className="spine-title" style={{ fontSize: 13, marginTop: 2 }}>{e.title}</div>
              </div>
              <Status status={e.status} />
            </div>
          ))}
        </div>
      )}

      {focusId && (
        <div className="spine">
          <div className="spine-rail"></div>

          {/* ALL EPICS root crumb */}
          <div
            className="spine-node"
            style={{ opacity: 0.6 }}
            onClick={() => onFocus(null)}
          >
            <div className="marker"></div>
            <div className="spine-type">ROOT</div>
            <div className="spine-title">All epics</div>
          </div>

          {/* The path */}
          {path.map((node, i) => {
            const isFocused = node.id === focusId;
            const isAncestor = !isFocused;
            const cls =
              "spine-node " + (isFocused ? "is-focused" : "is-ancestor");
            return (
              <div key={node.id} className={cls} onClick={() => onFocus(node.id)}>
                <div className="marker"></div>
                <div className="spine-type">
                  {TYPE_LABEL[node.type]} · {node.id}
                </div>
                <div className="spine-title">{node.title}</div>
                {isFocused && (
                  <div className="spine-meta">
                    <Status status={node.status} />
                  </div>
                )}
              </div>
            );
          })}

          {/* Siblings of focus, indented */}
          {siblingsOfFocus.length > 0 && (
            <div style={{ paddingLeft: 28, marginTop: 4 }}>
              <div
                className="spine-type"
                style={{ paddingLeft: 12, marginBottom: 6 }}
              >
                SIBLINGS · {siblingsOfFocus.length}
              </div>
              <div className="siblings">
                {siblingsOfFocus.map((s) => (
                  <div
                    key={s.id}
                    className="sibling-line"
                    onClick={() => onFocus(s.id)}
                  >
                    <span className="st">{s.id}</span>
                    <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      {s.title}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      <SectionLabel num="02">SAVED VIEWS</SectionLabel>
      <div style={{ padding: "0 28px 24px", fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--muted)", lineHeight: 2 }}>
        <div>▸ in-flight only</div>
        <div>▸ blocked items</div>
        <div>▸ unassigned tasks</div>
        <div>▸ this sprint</div>
        <div style={{ color: "var(--faint)" }}>▸ + new view</div>
      </div>
    </div>
  );
}

/* ============================================================
   Center — Focus Lens
   ============================================================ */

function Breadcrumbs({ path, onFocus }) {
  return (
    <div className="crumbs">
      <span className="crumb" onClick={() => onFocus(null)}>
        <span className="crumb-type">ROOT</span>all
      </span>
      {path.map((n, i) => {
        const isLast = i === path.length - 1;
        return (
          <React.Fragment key={n.id}>
            <span className="crumb-sep">/</span>
            <span
              className={"crumb " + (isLast ? "current" : "")}
              onClick={() => !isLast && onFocus(n.id)}
            >
              <span className="crumb-type">{TYPE_LABEL[n.type]}</span>
              {n.title}
            </span>
          </React.Fragment>
        );
      })}
    </div>
  );
}

function LensCard({ node }) {
  if (!node) return null;
  const isTask = node.type === "task";

  // Compute KPI dynamically if not provided
  const kpi = node.kpi || (() => {
    const kids = getChildren(node.id);
    const done = kids.filter((k) => k.status === "done").length;
    const inflight = kids.filter((k) => k.status === "in-flight").length;
    return [
      { k: TYPE_LABEL[node.type === "epic" ? "feature" : (node.type === "feature" ? "story" : "task")] + "S", v: kids.length, mono: true },
      { k: "IN FLIGHT", v: inflight, mono: true },
      { k: "DONE", v: done, mono: true },
      { k: "SIZE", v: node.points ? sizeOf(node.points) : sizeDistribution(getChildren(node.id)), mono: true },
    ];
  })();

  return (
    <div className="lens">
      <div className="lens-watermark" aria-hidden="true">{node.id}</div>

      <div className="lens-head">
        <div style={{ flex: 1 }}>
          <div className="lens-type">
            {TYPE_LABEL[node.type]}
            <span className="id">· {node.id}</span>
          </div>
          <div className={"lens-title " + (isTask ? "task" : "")}>{node.title}</div>
          <div className="lens-summary">{node.summary}</div>
        </div>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 12, minWidth: 200 }}>
          <Status status={node.status} />
          {typeof node.progress === "number" && node.type !== "task" && (
            <div className="progress-line" style={{ width: 180 }}>
              <div className="progress-track">
                <div className="progress-fill" style={{ width: pctText(node.progress) }}></div>
              </div>
              <div className="progress-pct">{pctText(node.progress)}</div>
            </div>
          )}
          {node.points && (
            <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, letterSpacing: "0.12em", color: "var(--muted)" }}>
              SIZE · {sizeOf(node.points)}
            </div>
          )}
          {node.owner && (
            <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, letterSpacing: "0.12em", color: "var(--muted)" }}>
              OWNER · {node.owner}
            </div>
          )}
        </div>
      </div>

      {!isTask && (
        <div className="lens-stats">
          {kpi.map((s, i) => (
            <div className="lens-stat" key={i}>
              <div className="k">{s.k}</div>
              <div className={"v " + (s.mono ? "mono " : "") + (s.small ? "small" : "")}>{s.v}</div>
              {s.sub && <div className="sub">{s.sub}</div>}
            </div>
          ))}
        </div>
      )}

      {isTask && (
        <>
          <div className="actions">
            <button className="act primary">DISPATCH AGENT</button>
            <button className="act">+ ADD TO SPRINT</button>
            <button className="act">EDIT</button>
            <button className="act">BLOCK</button>
          </div>

          <div className="acceptance">
            <h3>Acceptance criteria · {node.acceptance.filter(a => a.done).length} / {node.acceptance.length}</h3>
            <ul>
              {node.acceptance.map((a, i) => (
                <li key={i} className={a.done ? "done" : ""}>
                  <span className="cbx"></span>
                  <span>{a.t}</span>
                </li>
              ))}
            </ul>
          </div>

          {node.context && (
            <div className="context">
              <div className="context-card">
                <h4>Context · Files</h4>
                <div className="files">
                  {(node.context.files || []).length === 0 && <div className="f">— no files attached</div>}
                  {(node.context.files || []).map((f, i) => (
                    <div key={i}><span className="h">▸</span> {f}</div>
                  ))}
                </div>
              </div>
              <div className="context-card">
                <h4>Related nodes</h4>
                <div className="files">
                  {(node.context.related || []).map((rid, i) => {
                    const n = D.byId[rid];
                    if (!n) return null;
                    return (
                      <div key={i}>
                        <span className="f">{TYPE_LABEL[n.type]}</span>{" "}
                        <span className="h">{n.id}</span>{" "}
                        <span style={{ color: "var(--ink-2)" }}>{n.title}</span>
                      </div>
                    );
                  })}
                  {(!node.context.related || !node.context.related.length) && (
                    <div className="f">— no related nodes</div>
                  )}
                </div>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function ChildGrid({ node, onFocus, onAddToSprint, sprintQueue, density }) {
  if (!node) return null;
  const children = getChildren(node.id);

  if (children.length === 0) return null;

  const childType = children[0].type;
  const heading = TYPE_LABEL[childType] + "S";
  const isTaskGrid = childType === "task";

  return (
    <div>
      <div className="children-head">
        <h2>
          {heading}
          <span className="count">{children.length}</span>
        </h2>
        <div className="filters">
          <span className="on">ALL</span>
          <span>IN FLIGHT</span>
          <span>QUEUED</span>
          <span>BLOCKED</span>
          <span>DONE</span>
        </div>
      </div>

      <div className={"child-grid" + (density === "list" ? " list" : "")}>
        {children.map((c) => {
          const inSprint = sprintQueue.includes(c.id);
          return (
            <div
              key={c.id}
              className={"child " + c.type + (isTaskGrid ? " draggable" : "")}
              draggable={isTaskGrid}
              onDragStart={(e) => {
                if (!isTaskGrid) return;
                e.dataTransfer.setData("text/plain", c.id);
                e.dataTransfer.effectAllowed = "copy";
              }}
              onClick={() => onFocus(c.id)}
            >
              <div className="child-head">
                <span className="child-type">{TYPE_LABEL[c.type]}</span>
                <span className="child-id">{c.id}</span>
              </div>
              <div className="child-title">{c.title}</div>
              {!isTaskGrid && <div className="child-sum">{c.summary}</div>}

              <div className="child-foot">
                <Status status={c.status} />
                <div className="pcount">
                  {c.points && <span>{sizeOf(c.points)}</span>}
                  {c.children && c.children.length > 0 && (
                    <span>{c.children.length} {TYPE_LABEL[D.byId[c.children[0]].type]}S</span>
                  )}
                  {c.assignedAgent && (
                    <span style={{ color: "var(--accent)" }}>◆ {c.assignedAgent}</span>
                  )}
                  {isTaskGrid && !inSprint && (
                    <span
                      style={{ color: "var(--muted)", cursor: "pointer", textTransform: "uppercase" }}
                      onClick={(e) => { e.stopPropagation(); onAddToSprint(c.id); }}
                    >
                      + SPRINT
                    </span>
                  )}
                  {isTaskGrid && inSprint && (
                    <span style={{ color: "var(--accent)", textTransform: "uppercase" }}>◉ IN SPRINT</span>
                  )}
                </div>
              </div>

              {typeof c.progress === "number" && c.progress > 0 && c.status !== "done" && (
                <div className="child-progress">
                  <div style={{ width: pctText(c.progress) }}></div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function CenterToolbar({ view, onView, focusId }) {
  const path = getPathTo(focusId);
  const node = focusId ? D.byId[focusId] : null;
  return (
    <div className="center-toolbar">
      <div className="tag">
        {view === "focus"
          ? (node ? `${TYPE_LABEL[node.type]} · ${node.id}` : "PORTFOLIO")
          : "PLANNING GRAPH · FULL TREE"}
      </div>
      <div className="view-toggle">
        <button
          className={view === "focus" ? "on" : ""}
          onClick={() => onView("focus")}
        >◉ FOCUS</button>
        <button
          className={view === "tree" ? "on" : ""}
          onClick={() => onView("tree")}
        >⫶⫶ TREE</button>
      </div>
    </div>
  );
}

function FocusLensBody({ focusId, onFocus, onAddToSprint, sprintQueue }) {
  const node = focusId ? D.byId[focusId] : null;
  const path = useMemo(() => getPathTo(focusId), [focusId]);
  const isTask = node && node.type === "task";

  if (!node) {
    // Root view — show portfolio summary
    return (
      <div className="col" style={{ flex: 1, minHeight: 0 }}>
        <div className="center-inner">
          <Breadcrumbs path={[]} onFocus={onFocus} />
          <div className="lens">
            <div className="lens-watermark" aria-hidden="true">L</div>
            <div className="lens-head">
              <div style={{ flex: 1 }}>
                <div className="lens-type">PORTFOLIO <span className="id">· LUMINA / ALL</span></div>
                <div className="lens-title">Plan. Dispatch. Observe.</div>
                <div className="lens-summary">
                  This is the control surface for the agentic harness. Build out epics and features as the durable structure;
                  let sprints and tasks come and go through them. Drill into any node on the left to focus the lens.
                </div>
              </div>
              <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 12 }}>
                <Status status="in-flight" />
              </div>
            </div>
            <div className="lens-stats">
              <div className="lens-stat">
                <div className="k">EPICS</div>
                <div className="v mono">{D.epics.length}</div>
                <div className="sub">{D.epics.filter(e => e.status === "in-flight").length} IN FLIGHT</div>
              </div>
              <div className="lens-stat">
                <div className="k">FEATURES</div>
                <div className="v mono">{D.features.length}</div>
                <div className="sub">ACROSS PORTFOLIO</div>
              </div>
              <div className="lens-stat">
                <div className="k">STORIES</div>
                <div className="v mono">{D.stories.length}</div>
                <div className="sub">{D.stories.filter(s => s.status === "blocked").length} BLOCKED</div>
              </div>
              <div className="lens-stat">
                <div className="k">TASKS</div>
                <div className="v mono">{D.tasks.length}</div>
                <div className="sub">{D.tasks.filter(t => t.status === "in-flight").length} EXECUTING</div>
              </div>
            </div>
          </div>

          <div className="children-head">
            <h2>EPICS <span className="count">{D.epics.length}</span></h2>
            <div className="filters">
              <span className="on">ALL</span>
              <span>IN FLIGHT</span>
              <span>QUEUED</span>
              <span>DONE</span>
            </div>
          </div>

          <div className="child-grid">
            {D.epics.map((e) => (
              <div key={e.id} className="child epic" onClick={() => onFocus(e.id)} style={{ minHeight: 160 }}>
                <div className="child-head">
                  <span className="child-type">EPIC</span>
                  <span className="child-id">{e.id}</span>
                </div>
                <div className="child-title">{e.title}</div>
                <div className="child-sum">{e.summary}</div>
                <div className="child-foot">
                  <Status status={e.status} />
                  <div className="pcount">
                    <span>{e.children.length} FEAT</span>
                    <span style={{ color: "var(--accent)" }}>{pctText(e.progress)}</span>
                  </div>
                </div>
                {e.progress > 0 && (
                  <div className="child-progress">
                    <div style={{ width: pctText(e.progress) }}></div>
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="col" style={{ flex: 1, minHeight: 0 }}>
      <div className="center-inner">
        <Breadcrumbs path={path} onFocus={onFocus} />
        <LensCard node={node} />
        {!isTask && (
          <ChildGrid
            node={node}
            onFocus={onFocus}
            onAddToSprint={onAddToSprint}
            sprintQueue={sprintQueue}
          />
        )}
      </div>
    </div>
  );
}

/* ============================================================
   Right rail — Sprint composer + Agent stream
   ============================================================ */

function SprintComposer({ sprint, queue, onAddToSprint, onRemoveFromSprint, onDispatch }) {
  const [over, setOver] = useState(false);
  const queueSizes = sizeDistribution(queue.map((tid) => D.byId[tid]).filter(Boolean));

  return (
    <div className="right-section">
      <SectionLabel num="04">ACTIVE SPRINT</SectionLabel>
      <div className="sprint-head">
        <div>
          <div style={{ fontSize: 13.5, color: "var(--ink)", fontWeight: 500, lineHeight: 1.2 }}>
            {sprint.name}
          </div>
          <div className="sprint-meta" style={{ marginTop: 3 }}>
            {sprint.range} · {queue.length} / {sprint.capacity} ITEMS · {queueSizes}
          </div>
        </div>
        <Status status="in-flight" />
      </div>

      <div
        className={"sprint-zone " + (over ? "over" : "")}
        onDragOver={(e) => { e.preventDefault(); setOver(true); }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          const id = e.dataTransfer.getData("text/plain");
          if (id) onAddToSprint(id);
        }}
      >
        {queue.length === 0 && (
          <div className="empty">DRAG · TASK · HERE</div>
        )}
        {queue.map((tid) => {
          const t = D.byId[tid];
          if (!t) return null;
          return (
            <div key={tid} className="sprint-item">
              <div className="sid">{t.id}</div>
              <div className="stitle">{t.title}</div>
              <div className="sremove" onClick={() => onRemoveFromSprint(tid)}>×</div>
            </div>
          );
        })}
      </div>

      <button
        className="dispatch"
        disabled={queue.length === 0}
        onClick={onDispatch}
      >
        ▸ DISPATCH SPRINT · {queue.length} TASK{queue.length === 1 ? "" : "S"}
      </button>
    </div>
  );
}

function AgentList({ agents }) {
  const active = agents.filter((a) => a.status === "active").length;
  return (
    <div className="right-section flex">
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <SectionLabel num="05">AGENT STREAM</SectionLabel>
        <div style={{ paddingRight: 20, fontFamily: "var(--font-mono)", fontSize: 10.5, color: "var(--accent)", letterSpacing: "0.1em" }}>
          {active} / {agents.length} ACTIVE
        </div>
      </div>

      <div className="agent-list">
        {agents.map((a) => (
          <div key={a.id} className={"agent " + (a.status !== "active" ? a.status : "active")}>
            <div className="agent-row">
              <div className="agent-name">
                <span className="ico"></span>
                {a.name}
              </div>
              <div className="agent-time">{a.runtime}</div>
            </div>

            <div className="agent-task" style={{ color: a.status === "active" ? "var(--ink-2)" : "var(--muted)" }}>
              {a.currentTask && (
                <span style={{ fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--faint)", letterSpacing: "0.08em", marginRight: 8 }}>
                  {a.currentTask}
                </span>
              )}
              {a.currentTaskTitle}
            </div>

            <div className="agent-log">
              {a.log.slice(-3).map((l, i) => (
                <div key={i} className="line">
                  <span className="ts">{String(i + 12).padStart(2, "0")}:{String(34 + i * 7 % 26).padStart(2, "0")}</span>
                  {"  "}
                  <span className={"tag " + (l.level || "")}>{l.tag}</span>
                  {"  "}
                  <span>{l.text}</span>
                </div>
              ))}
            </div>

            {a.status === "active" && (
              <div className={"agent-progress " + (a.progress < 0.1 ? "indet" : "")}>
                <div style={{ width: pctText(Math.max(0.04, a.progress)) }}></div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

/* ============================================================
   Footer
   ============================================================ */

function Footer({ focusId }) {
  return (
    <div className="footer">
      <div className="left-set">
        <span className="kbd"><b>↑↓</b> NAV</span>
        <span className="kbd"><b>↵</b> FOCUS</span>
        <span className="kbd"><b>⌫</b> UP</span>
        <span className="kbd"><b>S</b> SPRINT</span>
        <span className="kbd"><b>D</b> DISPATCH</span>
      </div>
      <div className="right-set">
        <span>FOCUS · {focusId || "ROOT"}</span>
        <span>·</span>
        <span style={{ color: "var(--accent)" }}>SYNCED</span>
        <span>·</span>
        <span>HARNESS-CLUSTER / EU-WEST</span>
      </div>
    </div>
  );
}

/* ============================================================
   App
   ============================================================ */

function App() {
  const [focusId, setFocusId] = useState("S-001"); // start focused on a story to show off depth
  const [view, setView] = useState("focus"); // 'focus' | 'tree'
  const [sprintQueue, setSprintQueue] = useState(D.sprint.queue);
  const [flash, setFlash] = useState(null);
  const [agents, setAgents] = useState(D.agents);

  // Simulated live agent progress
  useEffect(() => {
    const id = setInterval(() => {
      setAgents((prev) =>
        prev.map((a) => {
          if (a.status !== "active") return a;
          const next = Math.min(0.985, a.progress + (Math.random() * 0.012 + 0.002));
          // bump runtime by ~3s
          const [h, m, s] = a.runtime.split(":").map((n) => parseInt(n, 10));
          let totalSec = h * 3600 + m * 60 + s + 3;
          const nh = Math.floor(totalSec / 3600);
          const nm = Math.floor((totalSec % 3600) / 60);
          const ns = totalSec % 60;
          const rt = `${String(nh).padStart(2, "0")}:${String(nm).padStart(2, "0")}:${String(ns).padStart(2, "0")}`;
          return { ...a, progress: next, runtime: rt };
        })
      );
    }, 3000);
    return () => clearInterval(id);
  }, []);

  // Keyboard nav
  useEffect(() => {
    const onKey = (e) => {
      if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
      if (e.key === "Backspace") {
        e.preventDefault();
        const cur = focusId ? D.byId[focusId] : null;
        if (cur && cur.parent) setFocusId(cur.parent);
        else if (cur) setFocusId(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focusId]);

  const handleAddToSprint = useCallback((id) => {
    setSprintQueue((q) => {
      if (q.includes(id)) return q;
      const t = D.byId[id];
      setFlash(`+ ${t.id} · ADDED TO SPRINT`);
      setTimeout(() => setFlash(null), 1400);
      return [...q, id];
    });
  }, []);

  const handleRemoveFromSprint = useCallback((id) => {
    setSprintQueue((q) => q.filter((x) => x !== id));
  }, []);

  const handleDispatch = useCallback(() => {
    setFlash(`▸ DISPATCHED ${sprintQueue.length} TASK${sprintQueue.length === 1 ? "" : "S"} TO HARNESS`);
    setTimeout(() => setFlash(null), 1800);
  }, [sprintQueue.length]);

  const activeAgents = agents.filter((a) => a.status === "active").length;

  const [tw, setTweak] = window.useTweaks
    ? window.useTweaks(TWEAK_DEFAULTS)
    : [TWEAK_DEFAULTS, () => {}];

  // Apply watermark CSS variables / visibility at the document level
  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty("--lens-watermark-display", tw.showWatermark ? "block" : "none");
    root.style.setProperty("--lens-watermark-opacity", String(tw.watermarkOpacity));
    root.style.setProperty("--lens-watermark-size", `${tw.watermarkSize}px`);
  }, [tw.showWatermark, tw.watermarkOpacity, tw.watermarkSize]);

  const {
    TweaksPanel,
    TweakSection,
    TweakToggle,
    TweakSlider,
  } = window;

  return (
    <div className="app">
      <Header sprint={D.sprint} activeAgents={activeAgents} />

      <div className="body">
        <HierarchySpine focusId={focusId} onFocus={setFocusId} />

        <div className={"col center " + (view === "tree" ? "tree-mode" : "")}>
          <CenterToolbar view={view} onView={setView} focusId={focusId} />
          {view === "focus" && (
            <FocusLensBody
              focusId={focusId}
              onFocus={setFocusId}
              onAddToSprint={handleAddToSprint}
              sprintQueue={sprintQueue}
            />
          )}
          {view === "tree" && window.TreeView &&
            React.createElement(window.TreeView, {
              focusId,
              onFocus: (id) => setFocusId(id),
              sprintQueue,
            })
          }
        </div>

        <div className="col right">
          <SprintComposer
            sprint={D.sprint}
            queue={sprintQueue}
            onAddToSprint={handleAddToSprint}
            onRemoveFromSprint={handleRemoveFromSprint}
            onDispatch={handleDispatch}
          />
          <AgentList agents={agents} />
        </div>
      </div>

      <Footer focusId={focusId} />

      {flash && <div className="flash">{flash}</div>}

      {TweaksPanel && (
        <TweaksPanel>
          <TweakSection label="Lens watermark" />
          <TweakToggle
            label="Show item code behind text"
            value={tw.showWatermark}
            onChange={(v) => setTweak("showWatermark", v)}
          />
          <TweakSlider
            label="Opacity"
            value={tw.watermarkOpacity}
            min={0}
            max={0.2}
            step={0.005}
            disabled={!tw.showWatermark}
            onChange={(v) => setTweak("watermarkOpacity", v)}
          />
          <TweakSlider
            label="Size"
            value={tw.watermarkSize}
            min={80}
            max={360}
            step={4}
            unit="px"
            disabled={!tw.showWatermark}
            onChange={(v) => setTweak("watermarkSize", v)}
          />
        </TweaksPanel>
      )}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<App />);
