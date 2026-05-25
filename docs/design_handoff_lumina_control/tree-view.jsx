/* LUMINA — Tree View
   Family-tree style visualization of the planning graph.
   Top-down: Epic → Feature → Story → Task
   Click a node to focus on it (switches back to focus mode).
*/

const { useState: useTreeState, useMemo: useTreeMemo, useRef: useTreeRef, useEffect: useTreeEffect } = React;

const TV = {
  W: { epic: 220, feature: 210, story: 210, task: 130 },
  H: { epic: 92, feature: 100, story: 108, task: 50 },
  GAP_X: 18,
  EPIC_GAP: 60,
  LEVEL_Y: { epic: 16, feature: 144, story: 280, task: 410 },
};

function layoutTree(selectedEpicId) {
  const cursor = { x: 0 };

  function lay(node, depth) {
    const W = TV.W[node.type];
    const H = TV.H[node.type];

    // Tree view only shows epic → feature → story (tasks summarized on story tiles)
    let childIds = (node.children || []).filter((cid) => {
      const c = window.LUMINA_DATA.byId[cid];
      return c && c.type !== "task";
    });
    const children = childIds.map((cid) => window.LUMINA_DATA.byId[cid]).filter(Boolean);

    if (children.length === 0) {
      const x = cursor.x;
      cursor.x += W + TV.GAP_X;
      return { id: node.id, node, x, y: TV.LEVEL_Y[node.type], w: W, h: H, children: [] };
    }

    const laidChildren = children.map((c) => lay(c, depth + 1));
    const first = laidChildren[0];
    const last = laidChildren[laidChildren.length - 1];
    const center = (first.x + last.x + last.w) / 2;
    const x = center - W / 2;

    return { id: node.id, node, x, y: TV.LEVEL_Y[node.type], w: W, h: H, children: laidChildren };
  }

  const epics = selectedEpicId
    ? window.LUMINA_DATA.epics.filter((e) => e.id === selectedEpicId)
    : window.LUMINA_DATA.epics;

  const trees = [];
  for (const epic of epics) {
    const t = lay(epic, 0);
    trees.push(t);
    cursor.x += TV.EPIC_GAP;
  }
  return { trees, totalW: Math.max(cursor.x, 200) };
}

function flattenTree(trees) {
  const nodes = [];
  const edges = [];
  function walk(t) {
    nodes.push(t);
    for (const c of t.children) {
      edges.push({ from: t, to: c });
      walk(c);
    }
  }
  for (const t of trees) walk(t);
  return { nodes, edges };
}

function connectorPath(from, to) {
  const x1 = from.x + from.w / 2;
  const y1 = from.y + from.h;
  const x2 = to.x + to.w / 2;
  const y2 = to.y;
  const midY = y1 + (y2 - y1) / 2;
  // orthogonal: down, across, down
  return `M ${x1} ${y1} V ${midY} H ${x2} V ${y2}`;
}

function TreeNode({ t, isFocused, isOnPath, inSprint, onClick }) {
  const n = t.node;
  const cls = [
    "tv-node",
    "tv-" + n.type,
    isFocused ? "is-focused" : "",
    isOnPath ? "is-on-path" : "",
    inSprint ? "in-sprint" : "",
    n.status === "done" ? "is-done" : "",
    n.status === "blocked" ? "is-blocked" : "",
    n.status === "in-flight" ? "is-active" : "",
  ].join(" ");

  // Compute a task summary for story nodes (and feature nodes one level up)
  let taskSummary = null;
  let emptyMsg = null;
  if (n.type === "story") {
    const kids = (n.children || []).map((cid) => window.LUMINA_DATA.byId[cid]).filter(Boolean);
    if (kids.length > 0) {
      const inflight = kids.filter((k) => k.status === "in-flight").length;
      const done = kids.filter((k) => k.status === "done").length;
      const blocked = kids.filter((k) => k.status === "blocked").length;
      taskSummary = { total: kids.length, inflight, done, blocked };
    } else {
      emptyMsg = "BREAKDOWN PENDING";
    }
  } else if (n.type === "feature") {
    const stories = (n.children || []).map((cid) => window.LUMINA_DATA.byId[cid]).filter(Boolean);
    let total = 0, inflight = 0, done = 0;
    for (const s of stories) {
      const tk = (s.children || []).map((cid) => window.LUMINA_DATA.byId[cid]).filter(Boolean);
      total += tk.length;
      inflight += tk.filter((k) => k.status === "in-flight").length;
      done += tk.filter((k) => k.status === "done").length;
    }
    if (total > 0) taskSummary = { total, inflight, done, blocked: 0, label: "TASK" };
    else emptyMsg = stories.length > 0 ? "AWAITING BREAKDOWN" : "NO STORIES YET";
  } else if (n.type === "epic") {
    const features = (n.children || []).map((cid) => window.LUMINA_DATA.byId[cid]).filter(Boolean);
    let total = 0, inflight = 0, done = 0;
    for (const f of features) {
      const stories = (f.children || []).map((cid) => window.LUMINA_DATA.byId[cid]).filter(Boolean);
      for (const s of stories) {
        const tk = (s.children || []).map((cid) => window.LUMINA_DATA.byId[cid]).filter(Boolean);
        total += tk.length;
        inflight += tk.filter((k) => k.status === "in-flight").length;
        done += tk.filter((k) => k.status === "done").length;
      }
    }
    if (total > 0) taskSummary = { total, inflight, done, blocked: 0, label: "TASK" };
    else emptyMsg = features.length > 0 ? "AWAITING BREAKDOWN" : "NO FEATURES YET";
  }

  return (
    <div
      className={cls}
      style={{ left: t.x, top: t.y, width: t.w, height: t.h }}
      onClick={() => onClick(n.id)}
    >
      <div className="tv-head">
        <span className="tv-type">{n.type.toUpperCase()}</span>
        <span className="tv-id">{n.id}</span>
      </div>
      <div className="tv-title">{n.title}</div>

      {taskSummary && (
        <div className="tv-tasks">
          <span className="tv-tasks-total">{taskSummary.total} TASKS</span>
          {taskSummary.inflight > 0 && (
            <span className="tv-tasks-pip in-flight" title={taskSummary.inflight + " in flight"}>
              <i></i>{taskSummary.inflight}
            </span>
          )}
          {taskSummary.done > 0 && (
            <span className="tv-tasks-pip done" title={taskSummary.done + " done"}>
              <i></i>{taskSummary.done}
            </span>
          )}
          {taskSummary.blocked > 0 && (
            <span className="tv-tasks-pip blocked" title={taskSummary.blocked + " blocked"}>
              <i></i>{taskSummary.blocked}
            </span>
          )}
        </div>
      )}

      {!taskSummary && emptyMsg && (
        <div className="tv-tasks tv-tasks-empty">
          <span>· {emptyMsg} ·</span>
        </div>
      )}

      <div className="tv-foot">
        {n.type !== "task" && typeof n.progress === "number" && (
          <span className="tv-pct">{Math.round((n.progress || 0) * 100)}%</span>
        )}
        {n.assignedAgent && <span className="tv-agent">◆ {n.assignedAgent}</span>}
        {inSprint && <span className="tv-sprint">◉ SPRINT</span>}
        {n.points && n.type === "story" && <span>{n.points <= 2 ? "S" : n.points <= 5 ? "M" : n.points <= 8 ? "L" : "XL"}</span>}
      </div>
      {typeof n.progress === "number" && n.progress > 0 && n.status !== "done" && (
        <div className="tv-progress"><div style={{ width: Math.round((n.progress || 0) * 100) + "%" }}></div></div>
      )}
    </div>
  );
}

function TreeView({ focusId, onFocus, sprintQueue }) {
  // Which epic's subtree to show. Default to the focused node's epic, else first epic.
  const initialEpic = (() => {
    if (focusId) {
      const path = [];
      let cur = window.LUMINA_DATA.byId[focusId];
      while (cur) { path.push(cur); cur = cur.parent ? window.LUMINA_DATA.byId[cur.parent] : null; }
      const epic = path.find((n) => n.type === "epic");
      if (epic) return epic.id;
    }
    return window.LUMINA_DATA.epics[0].id;
  })();
  const [selectedEpicId, setSelectedEpicId] = useTreeState(initialEpic);
  const [zoom, setZoom] = useTreeState(1.0);
  const scrollRef = useTreeRef(null);
  const innerRef = useTreeRef(null);

  const { trees, totalW } = useTreeMemo(() => layoutTree(selectedEpicId), [selectedEpicId]);
  const { nodes, edges } = useTreeMemo(() => flattenTree(trees), [trees]);

  const maxY = useTreeMemo(() => {
    let y = 0;
    for (const n of nodes) y = Math.max(y, n.y + n.h);
    return y + 40;
  }, [nodes]);

  // Highlight the path to focus
  const onPathIds = useTreeMemo(() => {
    if (!focusId) return new Set();
    const set = new Set();
    let cur = window.LUMINA_DATA.byId[focusId];
    while (cur) {
      set.add(cur.id);
      cur = cur.parent ? window.LUMINA_DATA.byId[cur.parent] : null;
    }
    return set;
  }, [focusId]);

  const sprintSet = useTreeMemo(() => new Set(sprintQueue || []), [sprintQueue]);

  function fit() {
    if (!scrollRef.current) return;
    const cw = scrollRef.current.clientWidth - 40;
    const ch = scrollRef.current.clientHeight - 40;
    const zx = cw / totalW;
    const zy = ch / maxY;
    // Floor at 0.7 — below that text is unreadable; user can scroll instead
    const z = Math.max(0.7, Math.min(1.0, Math.min(zx, zy)));
    setZoom(z);
    // Center the canvas horizontally
    requestAnimationFrame(() => {
      if (!scrollRef.current) return;
      const scaledW = totalW * z;
      const sw = scrollRef.current.clientWidth;
      scrollRef.current.scrollLeft = Math.max(0, (scaledW - sw) / 2);
      scrollRef.current.scrollTop = 0;
    });
  }

  // When focusId changes to a node in a different epic, switch the tab
  useTreeEffect(() => {
    if (!focusId) return;
    const path = [];
    let cur = window.LUMINA_DATA.byId[focusId];
    while (cur) { path.push(cur); cur = cur.parent ? window.LUMINA_DATA.byId[cur.parent] : null; }
    const epic = path.find((n) => n.type === "epic");
    if (epic && epic.id !== selectedEpicId) setSelectedEpicId(epic.id);
  }, [focusId]);

  // Fit on mount / epic change so the whole subtree is visible
  useTreeEffect(() => {
    const id = setTimeout(fit, 50);
    return () => clearTimeout(id);
    // eslint-disable-next-line
  }, [selectedEpicId]);

  // Center the focused node only when focusId itself changes (not on zoom/layout changes)
  const lastFocusedRef = useTreeRef(null);
  useTreeEffect(() => {
    if (lastFocusedRef.current === focusId) return;
    const isFirst = lastFocusedRef.current === null;
    lastFocusedRef.current = focusId;
    if (isFirst) return; // don't auto-scroll on mount
    if (!focusId || !scrollRef.current) return;
    const target = nodes.find((n) => n.id === focusId);
    if (!target) return;
    const cx = (target.x + target.w / 2) * zoom;
    const cy = (target.y + target.h / 2) * zoom;
    scrollRef.current.scrollTo({
      left: cx - scrollRef.current.clientWidth / 2,
      top: cy - scrollRef.current.clientHeight / 2,
      behavior: "smooth",
    });
  }, [focusId, zoom, nodes]);

  return (
    <div className="tv-wrap">
      <div className="tv-epic-tabs">
        {window.LUMINA_DATA.epics.map((e) => (
          <button
            key={e.id}
            className={"tv-epic-tab " + (selectedEpicId === e.id ? "on" : "")}
            onClick={() => setSelectedEpicId(e.id)}
          >
            <span className="tv-epic-tab-id">{e.id}</span>
            <span className="tv-epic-tab-title">{e.title}</span>
            <span className={"tv-epic-tab-dot " + e.status}></span>
          </button>
        ))}
      </div>

      <div className="tv-toolbar">
        <div className="tv-legend">
          <span className="tv-leg"><i className="d in-flight"></i> IN FLIGHT</span>
          <span className="tv-leg"><i className="d queued"></i> QUEUED</span>
          <span className="tv-leg"><i className="d done"></i> DONE</span>
          <span className="tv-leg"><i className="d blocked"></i> BLOCKED</span>
          <span className="tv-leg"><i className="d sprint"></i> IN SPRINT</span>
        </div>
        <div className="tv-controls">
          <div className="tv-zoom">
            <button className="tv-ctrl" onClick={() => setZoom((z) => Math.max(0.35, +(z - 0.1).toFixed(2)))}>−</button>
            <span className="tv-zoom-val">{Math.round(zoom * 100)}%</span>
            <button className="tv-ctrl" onClick={() => setZoom((z) => Math.min(1.4, +(z + 0.1).toFixed(2)))}>+</button>
            <button className="tv-ctrl" onClick={fit}>FIT</button>
          </div>
        </div>
      </div>

      <div className="tv-scroll" ref={scrollRef}>
        <div
          ref={innerRef}
          className="tv-canvas"
          style={{
            width: totalW * zoom + 80,
            height: maxY * zoom + 80,
          }}
        >
          <div
            className="tv-inner"
            style={{
              transform: `scale(${zoom})`,
              transformOrigin: "0 0",
              width: totalW + 80,
              height: maxY + 80,
            }}
          >
            <svg className="tv-edges" width={totalW + 80} height={maxY + 80}>
              {edges.map((e, i) => {
                const onPath = onPathIds.has(e.from.id) && onPathIds.has(e.to.id);
                return (
                  <path
                    key={i}
                    d={connectorPath(e.from, e.to)}
                    className={"tv-edge " + (onPath ? "on-path" : "")}
                  />
                );
              })}
            </svg>

            {/* Level labels (vertical guides) */}
            <div className="tv-level-labels">
              {["epic", "feature", "story"].map((k) => {
                return (
                  <div
                    key={k}
                    className="tv-level-label"
                    style={{ top: TV.LEVEL_Y[k] + TV.H[k] / 2 - 8 }}
                  >
                    {k.toUpperCase()}
                  </div>
                );
              })}
            </div>

            {nodes.map((t) => (
              <TreeNode
                key={t.id}
                t={t}
                isFocused={t.id === focusId}
                isOnPath={onPathIds.has(t.id) && t.id !== focusId}
                inSprint={t.node.type === "task" && sprintSet.has(t.id)}
                onClick={onFocus}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

window.TreeView = TreeView;
