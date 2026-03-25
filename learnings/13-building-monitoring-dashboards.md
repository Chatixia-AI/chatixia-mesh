# Lesson 13: Building Monitoring Dashboards

**Prerequisites:** [Lesson 07 -- Application Protocol Design](07-application-protocol-design.md), [Lesson 12 -- State Management Without a Database](12-state-management-without-a-database.md)

**Key files:**

| File | Purpose |
|------|---------|
| `hub/src/App.tsx` | Main component, polling orchestration, stat cards, layout |
| `hub/src/api.ts` | TypeScript API client (types, fetch wrappers) |
| `hub/src/theme.ts` | Design tokens (Atmospheric Luminescence system) |
| `hub/src/components/NetworkTopology.tsx` | Canvas-based mesh visualization |
| `hub/src/components/AgentCards.tsx` | Agent card grid with health indicators |
| `hub/src/components/TaskQueue.tsx` | Task table with expandable detail rows |
| `hub/src/components/ApprovalQueue.tsx` | Pairing approval flow |
| `hub/src/components/AgentChat.tsx` | User intervention interface |

---

## 1. Why Dashboards Matter

A distributed system without a dashboard is a distributed system you cannot operate. You can read logs, tail metrics, and write scripts that parse JSON -- but none of that tells you the current state of the system at a glance. Dashboards exist for humans.

### The Three Pillars of Observability

The industry talks about three pillars of observability:

1. **Metrics** -- Numeric measurements over time. How many agents are online? How many tasks are pending? What is the heartbeat age?
2. **Logs** -- Structured records of events. "Agent X registered at 14:32:01." "Task Y failed with error Z."
3. **Traces** -- The path a request takes through the system. Task submitted by hub-user, routed to agent-A, executed skill `summarize`, response returned.

chatixia-mesh's hub dashboard focuses primarily on metrics and system state. It answers the operator's first questions:

- Is the system healthy?
- Who is connected?
- What work is queued?
- What does the mesh topology look like?

These are not debugging questions -- they are operational awareness questions. A dashboard should answer them in under two seconds of looking at the screen.

### Dashboard vs. Monitoring vs. Alerting

These three concepts are distinct:

- **Dashboard** -- A visual interface for human consumption. Designed for at-a-glance comprehension.
- **Monitoring** -- Continuous observation of system health. Can be automated (Prometheus, Datadog) or manual (checking the dashboard).
- **Alerting** -- Automated notification when something goes wrong. PagerDuty, Slack webhooks, email.

chatixia-mesh currently implements only the dashboard layer. Monitoring and alerting would require persistent storage (time-series data) and notification infrastructure -- complexity the system does not yet need.

---

## 2. Polling vs. Push

The hub dashboard needs to stay current with system state. There are two fundamental approaches:

### How the Hub Polls

The hub uses the simplest possible strategy: poll the registry's REST API every 5 seconds. Here is the actual orchestration from `App.tsx`:

```tsx
const refresh = useCallback(async () => {
  try {
    const [a, t, topo, pending] = await Promise.all([
      fetchAgents(),
      fetchTasks(),
      fetchTopology(),
      fetchPendingApprovals(),
    ])
    setAgents(Array.isArray(a) ? a : [])
    setTasks(Array.isArray(t) ? t : [])
    setTopology(topo)
    setPendingApprovals(Array.isArray(pending) ? pending : [])
  } catch (e) {
    console.error('refresh error:', e)
  }
}, [])

useEffect(() => {
  refresh()
  const interval = setInterval(refresh, 5000)
  const tick = setInterval(() => setClock(
    new Date().toLocaleTimeString('en', { hour12: false })
  ), 1000)
  return () => { clearInterval(interval); clearInterval(tick) }
}, [refresh])
```

Key observations:

- **`Promise.all` parallelizes the four API calls.** The dashboard does not wait for agents to load before fetching tasks. All four requests fly concurrently.
- **Array safety:** `Array.isArray(a) ? a : []` guards against the registry returning unexpected shapes. Defensive coding for a live system.
- **Two intervals, different frequencies:** Data refreshes every 5 seconds. The clock ticks every 1 second. These are independent concerns with different update rates.
- **Cleanup function** returns from `useEffect` to clear both intervals when the component unmounts. This prevents memory leaks and stale timers.

The four API endpoints hit by each poll cycle:

```
GET /api/registry/agents       -> Agent[]
GET /api/hub/tasks/all          -> Task[]
GET /api/hub/network/topology   -> Topology
GET /api/pairing/pending        -> OnboardingEntry[]
```

### The Trade-offs

**Polling advantages:**

- No persistent connection to manage. Each poll is a stateless HTTP request.
- No reconnection logic. If a poll fails, the next one succeeds independently.
- Simple server implementation. The registry serves GET endpoints -- no WebSocket upgrade, no subscription tracking, no fan-out.
- Works through any HTTP proxy, CDN, or load balancer without configuration.

**Polling disadvantages:**

- **Staleness.** Data can be up to 5 seconds old. An agent could go offline 1 second after a poll, and the dashboard will not reflect it for another 4 seconds.
- **Wasted bandwidth.** Most polls return the same data. If nothing changed in the last 5 seconds, the entire response payload was unnecessary.
- **Fixed frequency.** You pick one interval for all data types. Agent health might warrant 2-second polls, while the approval queue could poll every 30 seconds.

### When WebSocket Push Is Worth It

A WebSocket (or Server-Sent Events) push model eliminates staleness: the server pushes updates the instant they happen. But it introduces complexity:

```
Polling model:
  Hub -> HTTP GET -> Registry -> HTTP response -> Hub
  (repeat every 5s)

Push model:
  Hub <-> WebSocket <-> Registry
  (persistent connection, server pushes on change)
```

Push is worth the complexity when:

1. **Latency matters.** Chat applications, trading dashboards, collaborative editing -- anything where "5 seconds stale" is unacceptable.
2. **Events are sparse.** If the system generates 1 event per minute, polling every 5 seconds wastes 11 out of 12 requests. Push only sends when something happens.
3. **You already have WebSocket infrastructure.** chatixia-mesh's registry already runs a WebSocket server for signaling. Adding a dashboard subscription channel would be incremental, not greenfield.

Push is not worth it when:

1. **Simplicity matters more than latency.** The hub is an operator tool, not an end-user product. 5-second staleness is acceptable.
2. **The number of dashboard clients is small.** One or two operators polling every 5 seconds is 0.2-0.4 requests/second per endpoint. This is negligible load.
3. **You want the dashboard to work without maintaining connection state.** A polling dashboard can be opened, closed, and reopened with no server-side impact.

chatixia-mesh chose polling deliberately. The registry already manages WebSocket connections for sidecar signaling, and adding dashboard subscriptions would interleave two very different concerns on the same WebSocket handler. Keeping them separate is a reasonable trade-off for a system with one or two simultaneous dashboard users.

---

## 3. Canvas-Based Topology Visualization

The most visually complex component in the hub is `NetworkTopology.tsx`. It renders agents as circles and DataChannel connections as lines on an HTML5 canvas.

### Why Canvas Instead of DOM or SVG?

Three rendering approaches exist for graph visualization on the web:

| Approach | Strengths | Weaknesses |
|----------|-----------|------------|
| DOM (divs) | Easy event handling, CSS styling | Slow with many elements, no drawing primitives |
| SVG | Declarative, stylable, accessible | Slower than canvas at scale, verbose markup |
| Canvas | Fast, full drawing control, good for custom visuals | Imperative API, no built-in event model |

The hub uses canvas because the topology is a custom visualization that needs gradients, glowing dots, dashed curved lines, and per-pixel control. SVG could achieve this but would require more markup. DOM divs cannot draw curves at all.

### Layout Strategy

The component uses two different layout strategies based on the number of nodes:

```
Small mesh (1-4 agents):

            REGISTRY
           /    |    \
          /     |     \
     agent-a  agent-b  agent-c
     (spread horizontally on lower band)


Large mesh (5+ agents):

              agent-e
             /       \
        agent-d       agent-a
            |  REGISTRY  |
        agent-c       agent-b
             \       /
              agent-f
     (circular arrangement, registry at center)
```

The layout decision is a single boolean:

```tsx
const smallLayout = nodes.length <= 4
```

For the small layout, agents are spread horizontally between 30% and 70% of the canvas width, on a band at 70% of the canvas height. The registry sits at the top center (50% width, 30% height). This gives a clean hierarchical look when there are few agents.

For the large layout, agents are distributed in a circle using trigonometry:

```tsx
const angle = (2 * Math.PI * i) / nodes.length - Math.PI / 2
return {
  x: hubX + Math.cos(angle) * circleRadius,
  y: hubY + Math.sin(angle) * circleRadius,
}
```

The `- Math.PI / 2` offset rotates the circle so the first node appears at the top (12 o'clock position) rather than the right (3 o'clock).

### Drawing Layers

The canvas draws in a specific order, from back to front:

1. **Background** -- Fill with `surfaceContainerLow` (light neutral surface).
2. **Hub-to-agent edges** -- Straight lines from registry to each agent. Drawn in `surfaceContainer` color (very subtle).
3. **Hub node** -- A gradient circle (dark teal to Electric Cyan) with a glow shadow. This is the visual anchor of the topology.
4. **Agent nodes** -- White circles with colored health dots inside. Each gets an ambient glow matching its health color.
5. **Mesh edges** -- Dashed curved lines between agents that have direct DataChannel connections. Drawn last so they appear on top.

### Mesh Edge Curves (Quadratic Bezier)

The mesh edges between agents are the most interesting drawing code. Rather than straight lines (which would overlap with the hub-to-agent edges), they use quadratic Bezier curves that bow toward the registry hub:

```tsx
// Compute a control point offset perpendicular to the edge
const mx = (from.x + to.x) / 2
const my = (from.y + to.y) / 2
const dx = to.x - from.x
const dy = to.y - from.y
const len = Math.sqrt(dx * dx + dy * dy) || 1

// Perpendicular unit vector
const px = -dy / len
const py = dx / len

// Pick the perpendicular direction closer to the hub
const toHubX = hubX - mx
const toHubY = hubY - my
const dot = px * toHubX + py * toHubY
const sign = dot >= 0 ? 1 : -1
const bowAmount = Math.min(len * 0.25, 50)

const cpx = mx + sign * px * bowAmount
const cpy = my + sign * py * bowAmount

ctx.quadraticCurveTo(cpx, cpy, to.x, to.y)
```

This algorithm:

1. Finds the midpoint between two agents.
2. Computes the perpendicular direction to the line between them.
3. Uses the dot product to determine which perpendicular direction points toward the hub.
4. Offsets the control point in that direction, creating a curve that bows inward.

The result is that mesh edges visually "wrap around" the hub node, making it clear they are peer-to-peer connections distinct from the hub-to-agent signaling lines.

### High-DPI Canvas Rendering

The component handles high-DPI (Retina) displays correctly:

```tsx
const dpr = window.devicePixelRatio || 1
canvas.width = W * dpr
canvas.height = H * dpr
canvas.style.width = `${W}px`
canvas.style.height = `${H}px`
ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
```

This is a common canvas pattern. The canvas element is sized in CSS at the logical pixel size but has its internal bitmap scaled by the device pixel ratio. The transform scales all drawing operations accordingly. Without this, canvas content would look blurry on Retina displays.

---

## 4. Design Systems and Design Tokens

### Why Centralized Design Tokens Matter

Every color, font, spacing value, and shadow in the hub dashboard comes from a single file: `theme.ts`. No component defines its own colors. No component invents its own spacing scale.

This is a design system -- a centralized set of constraints that keep the interface consistent. Without it, you get "design drift": one component uses `#333` for text, another uses `#2c2f31`, a third uses `rgb(50,50,50)`. They look similar but are not identical. Over time the UI becomes a patchwork.

Design tokens are the primitive values of a design system. They have semantic names that describe their purpose, not their appearance:

```ts
// Good: semantic names
color.onSurface    // Text on primary surface
color.onSurfaceMuted  // Secondary/muted text
color.active       // Health status: active
color.stale        // Health status: stale

// Bad: appearance names (do not do this)
color.darkGray
color.mediumGray
color.green
color.amber
```

Semantic names let you change the entire color scheme by updating one file. If you switch from light to dark theme, `color.surface` changes from `#f5f7f9` to `#1a1a1a`, and every component adapts automatically.

### The Atmospheric Luminescence System

chatixia-mesh's design system is called "Atmospheric Luminescence." The name describes its visual philosophy: surfaces emit subtle light rather than being bounded by hard edges.

The complete token set in `theme.ts`:

**Color palette:**

```ts
export const color = {
  surface: '#f5f7f9',               // Primary background
  surfaceContainerLow: '#eef1f3',   // Slightly darker surface
  surfaceContainer: '#e5e9eb',       // Component backgrounds
  surfaceContainerLowest: '#ffffff', // Brightest surface (inputs on focus)

  primary: '#00647b',               // Brand color (dark teal)
  primaryContainer: '#00cffc',       // Brand accent (Electric Cyan)
  onPrimary: '#ffffff',              // Text on primary
  onSurface: '#2c2f31',              // Primary text
  onSurfaceMuted: '#5f6368',         // Secondary text

  outlineVariant: 'rgba(171,173,175,0.15)',  // Ghost borders
  // ...
  active: '#16a34a',                 // Green -- agent is healthy
  stale: '#d97706',                  // Amber -- agent missed heartbeats
  offline: '#dc2626',                // Red -- agent is unreachable
}
```

**Typography:**

```ts
export const font = {
  display: "'Space Grotesk', sans-serif",  // Headings, labels
  body: "'Manrope', sans-serif",           // Body text
  mono: "'JetBrains Mono', monospace",     // Code, IDs, timestamps
}
```

Three font families serve three purposes. Display text uses Space Grotesk (geometric, clean). Body text uses Manrope (humanist, readable). Technical content uses JetBrains Mono (monospace, designed for code).

**Glassmorphism:**

```ts
export const glass = {
  header: {
    background: 'rgba(255,255,255,0.60)',
    backdropFilter: 'blur(32px)',
  },
  card: {
    background: 'rgba(255,255,255,0.80)',
    backdropFilter: 'blur(24px)',
  },
  overlay: {
    background: 'rgba(255,255,255,0.50)',
    backdropFilter: 'blur(24px)',
  },
}
```

Glassmorphism creates a frosted-glass effect. Semi-transparent backgrounds combined with `backdrop-filter: blur()` make content behind the element visible but blurred. Three intensities exist: the header is the most transparent (0.60), cards are mostly opaque (0.80), and overlays sit in between (0.50).

**Shadows:**

```ts
export const shadow = {
  ambient: '0 8px 40px rgba(44,47,49,0.06)',   // Subtle lift
  float: '0 12px 64px rgba(44,47,49,0.08)',    // Selected/hover state
  primaryGlow: '0 8px 32px rgba(0,207,252,0.18)', // Cyan glow for CTAs
}
```

Shadows use "ambient luminance" -- very soft, large-radius shadows with low opacity. This creates the feeling that elements float above the surface rather than sitting on it. The `primaryGlow` shadow adds a cyan tint, making primary action buttons appear to emit light.

### Tonal Surface Layering

Instead of using explicit borders to separate sections, the system uses subtle changes in surface color:

```
surfaceContainerLowest (#ffffff) -- brightest (focused input)
surface (#f5f7f9)                -- primary background
surfaceContainerLow (#eef1f3)    -- header rows, details
surfaceContainer (#e5e9eb)        -- code blocks, tags
```

Each layer is only slightly darker than the one above it. The `outlineVariant` is `rgba(171,173,175,0.15)` -- a 15% opacity gray that is barely visible. Borders exist but are nearly invisible, relying on the tonal difference between surfaces to create visual separation.

---

## 5. Component Architecture

### Data Flow

The hub uses a straightforward top-down data flow. `App.tsx` owns all state and passes it to child components as props:

```
App.tsx (state owner)
  |
  |-- agents ----> AgentCards (display)
  |-- tasks -----> TaskQueue (display)
  |-- topology --> NetworkTopology (display)
  |-- pendingApprovals -> ApprovalQueue (display + actions)
  |-- selectedAgent ----> AgentChat (display + actions)
```

Components do not fetch their own data. They receive it, render it, and optionally call back to the parent:

```
App.tsx
  |
  | props: agents[], onSelect callback
  v
AgentCards
  |
  | user clicks a card
  v
onSelect(agentId) -- sets selectedAgent in App.tsx
  |
  | selectedAgent is passed to AgentChat
  v
AgentChat renders for that agent
```

This is React's "lifting state up" pattern. It keeps data flow predictable: there is one source of truth (App.tsx), and all components render from it.

### Component Breakdown

**StatCard** -- Inline in `App.tsx`. A simple metric display: label (uppercase, small) and value (large number in accent color). Four of these form a grid at the top of the dashboard:

```
+------------------+------------------+------------------+------------------+
| AGENTS ONLINE    | TOTAL AGENTS     | PENDING TASKS    | AWAITING         |
|       3           |       5           |       2           | APPROVAL   0     |
+------------------+------------------+------------------+------------------+
```

The accent color carries meaning: green for active agents, teal for total, amber for pending items.

**AgentCards** -- A responsive grid of agent cards. Each card shows:

```
+-----------------------------------------+
| * agent-pi-summarizer         ONLINE    |
|                                         |
| host     pi-cluster-1                   |
| endpoint 192.168.0.145:8081             |
| peer     agent-pi-summarizer-sidecar    |
|                                         |
| [3 SKILLS] [autonomous]                |
+-----------------------------------------+
```

The health dot (`*`) glows with a box-shadow matching the health color. The card is clickable -- clicking it sets `selectedAgent` in the parent, which opens the AgentChat component.

Empty state is explicit: "Waiting for agent heartbeats..." is shown when the agents array is empty. This tells the operator why the list is empty (no heartbeats received) rather than just showing a blank space.

**TaskQueue** -- A table with expandable rows. The header row defines six columns: ID, State, Skill, Source, Target, Age. Clicking a row toggles a detail panel showing the full payload, error, result, timestamps, and TTL.

State badges use color coding: pending (amber), assigned (blue), completed (green), failed (red). Each state has a text color and a tinted background:

```ts
const stateStyles = {
  pending:   { color: '#d97706', bg: 'rgba(217,119,6,0.08)' },
  assigned:  { color: '#0284c7', bg: 'rgba(2,132,199,0.08)' },
  completed: { color: '#16a34a', bg: 'rgba(22,163,74,0.08)' },
  failed:    { color: '#dc2626', bg: 'rgba(220,38,38,0.08)' },
}
```

The `bg` values use 8% opacity of the text color. This creates a subtle tinted background that reinforces the state without overwhelming the table.

**ApprovalQueue** -- Renders only when there are pending approval entries. Each entry shows agent name, ID, peer ID, and age, with Approve and Reject buttons. The `onAction` callback triggers `refresh()` in the parent, which re-polls all data after an approval action.

A loading state disables both buttons and changes the cursor to `wait` while the approval request is in flight. This prevents double-submission.

**AgentChat** -- An intervention interface that appears below the agent cards when an agent is selected. It submits a task with skill `user_intervention` targeted at the selected agent:

```ts
await submitTask({
  skill: 'user_intervention',
  target_agent_id: agentId,
  source_agent_id: 'hub-user',
  payload: { message: message.trim() },
})
```

This reuses the existing task queue infrastructure. The "chat" is not a real-time conversation -- it is a task submission form. The agent processes it through its normal skill execution pipeline.

### The API Client

`api.ts` defines TypeScript interfaces for all data shapes and wraps `fetch` calls:

```ts
export interface Agent {
  agent_id: string
  hostname: string
  ip: string
  port: number
  sidecar_peer_id: string
  health: string
  mode: string
  capabilities: {
    skills: string[]
    mcp_servers: string[]
    goals_count: number
  }
  last_heartbeat: string
}
```

The base URL is empty (`const BASE = ''`), which means all API calls go to the same origin. In development, Vite proxies these to the registry. In production, the hub is served by the registry itself, so same-origin requests work natively.

Functions are thin wrappers:

```ts
export async function fetchAgents(): Promise<Agent[]> {
  const res = await fetch(`${BASE}/api/registry/agents`)
  return res.json()
}
```

No error handling at this layer -- errors propagate up to the `refresh` function in `App.tsx`, which catches and logs them. This keeps the API client simple and lets the caller decide what to do with failures.

---

## 6. Practical Considerations

### Handling Stale Data

Because the dashboard polls every 5 seconds, the displayed data is always slightly stale. The system handles this at multiple levels:

**Registry health checks** run every 15 seconds. They classify agents based on heartbeat age:

| Heartbeat age | Health status |
|---------------|---------------|
| < 90 seconds | `active` |
| 90 - 270 seconds | `stale` |
| > 270 seconds | `offline` |

These thresholds are generous -- an agent sending heartbeats every 15 seconds would need to miss 6 consecutive heartbeats before being marked stale.

**The dashboard does not track its own staleness.** It trusts the registry's health classification. If the poll fails, the dashboard silently retains the last known state. There is no "connection lost" banner, no retry counter. This is a simplicity trade-off: it avoids the complexity of tracking dashboard-level connection health, but it means the dashboard could show stale data indefinitely if the registry becomes unreachable.

### Empty States

Every component handles the "nothing to show" case explicitly:

| Component | Empty state message |
|-----------|-------------------|
| AgentCards | "Waiting for agent heartbeats..." |
| TaskQueue | "No tasks in queue" |
| ApprovalQueue | (component does not render at all) |
| NetworkTopology | "Waiting for agents to join the mesh..." |

The ApprovalQueue takes a different approach: it returns `null` when there are no entries. This means it occupies zero space in the layout. Since pending approvals are transient events (they exist only during the pairing window), removing them entirely when empty avoids a permanent "0 pending" placeholder.

### Relative Timestamps

The task queue uses a `formatAge` function to display how long ago a task was created:

```ts
function formatAge(epoch: number): string {
  const seconds = Math.floor(Date.now() / 1000 - epoch)
  if (seconds < 60) return `${seconds}s`
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`
  return `${Math.floor(seconds / 3600)}h`
}
```

This converts a Unix epoch timestamp into a human-readable relative time: `5s`, `3m`, `2h`. The function is deliberately simple -- no "days" or "weeks" because tasks in chatixia-mesh have TTLs measured in minutes, not days.

The ApprovalQueue has its own `formatAge` variant that appends "ago": `5s ago`, `3m ago`. This small difference in wording makes the timestamp read more naturally in the context of an approval card versus a table cell.

### Live Clock

The header displays a live clock that ticks every second:

```tsx
const [clock, setClock] = useState(() =>
  new Date().toLocaleTimeString('en', { hour12: false })
)

// In useEffect:
const tick = setInterval(() =>
  setClock(new Date().toLocaleTimeString('en', { hour12: false })),
  1000
)
```

The clock serves two purposes:

1. **Liveness indicator.** If the clock is ticking, the React app is alive and running JavaScript. If it freezes, something is wrong.
2. **Time reference.** Combined with relative timestamps ("5s ago"), the operator can mentally reconstruct absolute times without checking a separate clock.

The clock uses 24-hour format (`hour12: false`) -- standard for operational dashboards where AM/PM ambiguity is unwanted.

---

## Summary

The chatixia-mesh hub dashboard demonstrates several patterns common in monitoring interfaces:

- **Polling with `setInterval` + `Promise.all`** provides simple, reliable data freshness without WebSocket complexity.
- **Canvas rendering** gives full control over topology visualization, including gradients, glows, and Bezier curves.
- **Centralized design tokens** enforce visual consistency across every component.
- **Top-down data flow** keeps state management predictable: one owner, many readers.
- **Explicit empty states** prevent confusion when data is absent.
- **Relative timestamps** make temporal data scannable.

The system makes deliberate trade-offs: polling over push (simplicity over latency), canvas over SVG (control over accessibility), inline styles over CSS classes (co-location over separation). Each choice is defensible for a single-operator dashboard but might need revisiting if the dashboard grew into a multi-user, production-grade monitoring tool.

---

## Exercises

### Exercise 1: Calculate Offline Detection Latency

An agent's process crashes at time T=0. Walk through the full chain of events that must occur before the dashboard shows this agent as "offline." Calculate the worst-case total latency.

You will need these values from the codebase:

- Agent heartbeat interval: 15 seconds (`agent/chatixia/runner.py`, line 174)
- Registry health check loop interval: 15 seconds (`registry/src/registry.rs`, line 106)
- Registry "offline" threshold: heartbeat age > 270 seconds (`registry/src/registry.rs`, line 110)
- Hub poll interval: 5 seconds (`hub/src/App.tsx`, line 37)

Questions to answer:

1. At what point after T=0 does the last heartbeat become 270 seconds old (the offline threshold)?
2. What is the worst-case delay before the registry's health check loop runs after the threshold is crossed?
3. What is the worst-case delay before the hub polls after the registry updates the health status?
4. What is the total worst-case latency from agent crash to dashboard showing "offline"?
5. Now calculate the same chain for the "stale" status (threshold: 90 seconds). How long before the dashboard shows "stale"?
6. Is this acceptable for an operational dashboard? What would you change to detect failures faster, and what trade-offs would each change introduce?

### Exercise 2: Replace Polling with WebSocket Push

The hub currently polls every 5 seconds. Design a WebSocket-based push system that sends updates to connected dashboards in real time.

Address these questions:

1. **Registry changes:** The registry already has a WebSocket handler for sidecar signaling (`registry/src/signaling.rs`). Would you add dashboard subscriptions to the same WebSocket endpoint, or create a separate one? What are the trade-offs?
2. **Message format:** Design the JSON message format for dashboard push events. What event types do you need? (Hint: think about what the four polling endpoints return and when that data changes.)
3. **Connection lifecycle:** What happens when the dashboard opens? Does it receive a full state snapshot, or does it build state incrementally from events? What happens on reconnect after a network interruption?
4. **Hub changes:** How does the React code change? Replace `setInterval` with what? How do you handle the initial data load before the WebSocket connects?
5. **Is it worth it?** Given that chatixia-mesh typically has 1-2 dashboard users and the polling load is negligible, make the case for and against this change. Under what conditions would your answer change?

### Exercise 3: Design a Task Timeline View

The current TaskQueue component shows tasks in a table. Design a timeline component that visualizes the lifecycle of each task: created, assigned, executing, completed/failed.

Define the component:

1. **Data shape:** What TypeScript interface would you define for timeline entries? Consider that the current `Task` type has `created_at` and `updated_at` as Unix epochs, but does not have individual timestamps for each state transition. What API changes would be needed?
2. **Visual design:** Sketch an ASCII diagram of what a single task's timeline would look like. Show the horizontal time axis, state transition markers, and duration bars.
3. **Multiple tasks:** How would you stack multiple task timelines vertically? How do you handle tasks that overlap in time?
4. **Integration:** Where does this component fit in the App.tsx layout? Does it replace TaskQueue or complement it? What props does it receive?

Provide your timeline entry interface, an ASCII mockup of the visualization, and a brief description of how you would implement the rendering (canvas, SVG, or DOM elements -- and why).

### Exercise 4: Add a Latency Sparkline to Agent Cards

Each agent card currently shows hostname, endpoint, peer ID, and skill count. Design a "latency sparkline" -- a tiny inline chart showing the last N heartbeat round-trip times.

Address these questions:

1. **Data source:** The registry does not currently track heartbeat latency. Where would you measure it? (Hint: the agent sends heartbeats via `POST /api/hub/heartbeat`. What timestamp information is available at the registry when it receives a heartbeat?)
2. **API changes:** Design a new field on the `AgentRecord` struct (or a new endpoint) that exposes latency history. How many data points should you keep? What is the memory cost per agent?
3. **Visualization:** A sparkline is a tiny chart, typically 60-80 pixels wide and 20 pixels tall. Would you render it with canvas, SVG, or CSS? Implement the sparkline as a React component that takes an array of numbers and renders a line chart. Include the component signature and rendering logic.
4. **Integration:** Where in the agent card layout does the sparkline go? How does it interact with the existing health indicators? If latency spikes, should the sparkline change color?

---

**Next lesson:** [Lesson 14 -- Threat Modeling](14-threat-modeling.md)
