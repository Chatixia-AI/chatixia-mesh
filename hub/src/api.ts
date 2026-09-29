/// Registry API client for the hub dashboard.

const BASE = '';  // Same origin — proxied by Vite in dev

// ─── Admin token (ADR-024) ─────────────────────────────────────────────────
//
// Pairing admin routes and every write need the registry admin token, sent as
// the `x-admin-token` header. It lives in sessionStorage (cleared when the tab
// closes) and can be handed over in the URL fragment the registry logs at
// startup: http://host:8080/#admin_token=adm_...

const TOKEN_KEY = 'chatixia.adminToken';
export const ADMIN_LOCKED_EVENT = 'chatixia:admin-locked';

export class UnauthorizedError extends Error {
  constructor() {
    super('admin token required');
  }
}

export function getAdminToken(): string | null {
  try {
    return sessionStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function setAdminToken(token: string): void {
  try {
    sessionStorage.setItem(TOKEN_KEY, token.trim());
  } catch {
    // storage unavailable (private mode) — the token just won't persist
  }
}

export function clearAdminToken(): void {
  try {
    sessionStorage.removeItem(TOKEN_KEY);
  } catch {
    // ignore
  }
}

/** Move `#admin_token=...` from the URL into sessionStorage and strip it from the address bar. */
export function adoptAdminTokenFromUrl(): void {
  const match = window.location.hash.match(/admin_token=([^&]+)/);
  if (!match) return;
  setAdminToken(decodeURIComponent(match[1]));
  history.replaceState(null, '', window.location.pathname + window.location.search);
}

/** fetch with the admin token; a 401 clears the stored token and notifies the app. */
async function adminFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const token = getAdminToken();
  if (!token) throw new UnauthorizedError();
  const headers = new Headers(init.headers);
  headers.set('x-admin-token', token);
  const res = await fetch(`${BASE}${path}`, { ...init, headers });
  if (res.status === 401) {
    clearAdminToken();
    window.dispatchEvent(new Event(ADMIN_LOCKED_EVENT));
    throw new UnauthorizedError();
  }
  return res;
}

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

// ─── Pairing / Approval ────────────────────────────────────────────────────

export interface OnboardingEntry {
  id: string;
  agent_name: string;
  peer_id: string;
  device_token?: string;
  status: string;  // "pending_approval" | "approved" | "rejected" | "revoked"
  created_at: number;
  updated_at: number;
}

export async function fetchPendingApprovals(): Promise<OnboardingEntry[]> {
  const res = await adminFetch('/api/pairing/pending');
  return res.json();
}

export async function approveAgent(id: string): Promise<OnboardingEntry> {
  const res = await adminFetch(`/api/pairing/${id}/approve`, { method: 'POST' });
  return res.json();
}

export async function rejectAgent(id: string): Promise<OnboardingEntry> {
  const res = await adminFetch(`/api/pairing/${id}/reject`, { method: 'POST' });
  return res.json();
}

export async function revokeAgent(id: string): Promise<void> {
  await adminFetch(`/api/pairing/${id}/revoke`, { method: 'POST' });
}

export async function generateInviteCode(): Promise<{ code: string; expires_in: number }> {
  const res = await adminFetch('/api/pairing/generate-code', { method: 'POST' });
  return res.json();
}

// ─── Tasks ─────────────────────────────────────────────────────────────────

export async function submitTask(task: {
  skill?: string;
  target_agent_id: string;
  source_agent_id?: string;
  payload: Record<string, unknown>;
}): Promise<{ task_id: string }> {
  const res = await adminFetch('/api/hub/tasks', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(task),
  });
  return res.json();
}
