/// Registry API client for the hub dashboard.

const BASE = '';  // Same origin — proxied by Vite in dev

export interface Agent {
  agent_id: string;
  hostname: string;
  ip: string;
  port: number;
  sidecar_peer_id: string;
  health: string;
  mode: string;
  status: string;
  capabilities: {
    skills: string[];
    mcp_servers: string[];
    goals_count: number;
  };
  registered_at: string;
  last_heartbeat: string;
}

export interface Task {
  id: string;
  skill: string;
  target_agent_id: string;
  source_agent_id: string;
  assigned_agent_id: string;
  payload: Record<string, unknown>;
  state: string;
  result: string;
  error: string;
  created_at: number;
  updated_at: number;
  ttl: number;
}

export interface TopologyNode {
  agent_id: string;
  ip: string;
  port: number;
  hostname: string;
  sidecar_peer_id: string;
  mode: string;
  skills_count: number;
  health: string;
  mesh_peers: string[];
}

export interface Topology {
  nodes: TopologyNode[];
  mesh_edges: { from_peer: string; to_peer: string }[];
}

export async function fetchAgents(): Promise<Agent[]> {
  const res = await fetch(`${BASE}/api/registry/agents`);
  return res.json();
}

export async function fetchTasks(): Promise<Task[]> {
  const res = await fetch(`${BASE}/api/hub/tasks/all`);
  return res.json();
}

export async function fetchTopology(): Promise<Topology> {
  const res = await fetch(`${BASE}/api/hub/network/topology`);
  return res.json();
}

export async function submitTask(task: {
  skill?: string;
  target_agent_id: string;
  source_agent_id?: string;
  payload: Record<string, unknown>;
}): Promise<{ task_id: string }> {
  const res = await fetch(`${BASE}/api/hub/tasks`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(task),
  });
  return res.json();
}
