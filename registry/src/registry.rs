//! Agent registry — tracks agents, their skills, health, and capabilities.

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::Utc;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::AppState;

/// Agent registration payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent_id: String,
    pub hostname: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub sidecar_peer_id: String,
    #[serde(default)]
    pub capabilities: AgentCapabilities,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub mode: String,
}

fn default_port() -> u16 {
    8000
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentCapabilities {
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<String>,
    #[serde(default)]
    pub goals_count: u32,
    #[serde(default)]
    pub mode: String,
}

/// Internal agent record with metadata.
#[derive(Debug, Clone, Serialize)]
pub struct AgentRecord {
    #[serde(flatten)]
    pub info: AgentInfo,
    pub health: String,
    pub registered_at: String,
    pub last_heartbeat: String,
    #[serde(skip)]
    pub last_heartbeat_epoch: f64,
}

/// Heartbeat payload (from agent SDK).
#[derive(Debug, Deserialize)]
pub struct Heartbeat {
    pub agent_id: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub skills_count: u32,
    #[serde(default)]
    pub goals_count: u32,
    #[serde(default)]
    pub uptime_seconds: u64,
    #[serde(default)]
    pub skill_names: Vec<String>,
    #[serde(default)]
    pub sidecar_peer_id: String,
}

#[derive(Debug, Deserialize)]
pub struct RouteQuery {
    pub skill: String,
}

pub struct RegistryState {
    agents: DashMap<String, AgentRecord>,
}

impl RegistryState {
    pub fn new() -> Self {
        Self {
            agents: DashMap::new(),
        }
    }

    /// Background health check — marks agents as stale/offline.
    pub async fn health_check_loop(&self) {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let now = epoch_now();
            for mut entry in self.agents.iter_mut() {
                let age = now - entry.last_heartbeat_epoch;
                entry.health = if age > 270.0 {
                    "offline".into()
                } else if age > 90.0 {
                    "stale".into()
                } else {
                    "active".into()
                };
            }
        }
    }

    /// Get all agents as a list.
    pub fn list(&self) -> Vec<AgentRecord> {
        self.agents.iter().map(|e| e.value().clone()).collect()
    }

    /// Get a specific agent.
    pub fn get(&self, agent_id: &str) -> Option<AgentRecord> {
        self.agents.get(agent_id).map(|e| e.value().clone())
    }

    /// Find agents that have a specific skill.
    pub fn find_by_skill(&self, skill: &str) -> Vec<AgentRecord> {
        self.agents
            .iter()
            .filter(|e| {
                e.value().health == "active"
                    && e.value().info.capabilities.skills.contains(&skill.to_string())
            })
            .map(|e| e.value().clone())
            .collect()
    }
}

fn epoch_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

// ─── HTTP Handlers ───────────────────────────────────────────────────────

/// POST /api/registry/agents — register or update an agent.
pub async fn register_agent(
    State(state): State<AppState>,
    Json(info): Json<AgentInfo>,
) -> Json<serde_json::Value> {
    let now = Utc::now().to_rfc3339();
    let record = AgentRecord {
        info: info.clone(),
        health: "active".into(),
        registered_at: now.clone(),
        last_heartbeat: now,
        last_heartbeat_epoch: epoch_now(),
    };
    info!(
        "[REG] agent registered: {} (skills: {}, sidecar: {})",
        info.agent_id,
        info.capabilities.skills.len(),
        info.sidecar_peer_id
    );
    state.registry.agents.insert(info.agent_id.clone(), record);

    Json(serde_json::json!({ "status": "ok" }))
}

/// GET /api/registry/agents — list all agents.
pub async fn list_agents(State(state): State<AppState>) -> Json<Vec<AgentRecord>> {
    Json(state.registry.list())
}

/// DELETE /api/registry/agents/:agent_id — unregister an agent.
pub async fn delete_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Json<serde_json::Value> {
    if state.registry.agents.remove(&agent_id).is_some() {
        info!("[REG] agent unregistered: {}", agent_id);
        Json(serde_json::json!({ "status": "ok" }))
    } else {
        Json(serde_json::json!({ "error": "not found" }))
    }
}

/// GET /api/registry/agents/:agent_id — get a specific agent.
pub async fn get_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Json<serde_json::Value> {
    match state.registry.get(&agent_id) {
        Some(record) => Json(serde_json::to_value(record).unwrap()),
        None => Json(serde_json::json!({ "error": "not found" })),
    }
}

/// GET /api/registry/route?skill=X — find the best agent for a skill.
pub async fn route_by_skill(
    State(state): State<AppState>,
    Query(q): Query<RouteQuery>,
) -> Json<serde_json::Value> {
    let agents = state.registry.find_by_skill(&q.skill);
    if agents.is_empty() {
        return Json(serde_json::json!({
            "error": "no agent found with skill",
            "skill": q.skill
        }));
    }
    // Return first available (could add load balancing later)
    Json(serde_json::to_value(&agents[0]).unwrap())
}

/// POST /api/hub/heartbeat — receive heartbeat from agent (compat with chatixia-agent SDK).
pub async fn heartbeat(
    State(state): State<AppState>,
    Json(hb): Json<Heartbeat>,
) -> Json<serde_json::Value> {
    let now_str = Utc::now().to_rfc3339();
    let now_epoch = epoch_now();

    // Upsert agent record
    if let Some(mut entry) = state.registry.agents.get_mut(&hb.agent_id) {
        entry.last_heartbeat = now_str;
        entry.last_heartbeat_epoch = now_epoch;
        entry.health = "active".into();
        entry.info.hostname = hb.hostname.clone();
        entry.info.ip = hb.ip.clone();
        entry.info.port = hb.port;
        entry.info.status = hb.status.clone();
        entry.info.mode = hb.mode.clone();
        entry.info.capabilities.skills = hb.skill_names.clone();
        entry.info.capabilities.goals_count = hb.goals_count;
        if !hb.sidecar_peer_id.is_empty() {
            entry.info.sidecar_peer_id = hb.sidecar_peer_id.clone();
        }
    } else {
        // First heartbeat — register agent
        let info = AgentInfo {
            agent_id: hb.agent_id.clone(),
            hostname: hb.hostname.clone(),
            ip: hb.ip.clone(),
            port: hb.port,
            sidecar_peer_id: hb.sidecar_peer_id.clone(),
            capabilities: AgentCapabilities {
                skills: hb.skill_names.clone(),
                goals_count: hb.goals_count,
                mode: hb.mode.clone(),
                ..Default::default()
            },
            status: hb.status.clone(),
            mode: hb.mode.clone(),
        };
        state.registry.agents.insert(
            hb.agent_id.clone(),
            AgentRecord {
                info,
                health: "active".into(),
                registered_at: now_str.clone(),
                last_heartbeat: now_str,
                last_heartbeat_epoch: now_epoch,
            },
        );
    }

    // Find pending tasks for this agent
    let pending = state.hub.get_pending_for_agent(
        &hb.agent_id,
        &hb.skill_names,
    );

    Json(serde_json::json!({
        "status": "ok",
        "pending_tasks": pending
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_agent(id: &str, skills: Vec<String>, health: &str) -> AgentRecord {
        AgentRecord {
            info: AgentInfo {
                agent_id: id.to_string(),
                hostname: "localhost".into(),
                ip: "127.0.0.1".into(),
                port: default_port(),
                sidecar_peer_id: String::new(),
                capabilities: AgentCapabilities {
                    skills,
                    ..Default::default()
                },
                status: "running".into(),
                mode: "auto".into(),
            },
            health: health.to_string(),
            registered_at: "2025-01-01T00:00:00Z".into(),
            last_heartbeat: "2025-01-01T00:00:00Z".into(),
            last_heartbeat_epoch: epoch_now(),
        }
    }

    #[test]
    fn test_new_registry_is_empty() {
        let reg = RegistryState::new();
        assert!(reg.list().is_empty());
    }

    #[test]
    fn test_register_and_list() {
        let reg = RegistryState::new();
        let a1 = make_agent("a1", vec!["search".into()], "active");
        let a2 = make_agent("a2", vec!["chat".into()], "active");
        reg.agents.insert("a1".into(), a1);
        reg.agents.insert("a2".into(), a2);
        assert_eq!(reg.list().len(), 2);
    }

    #[test]
    fn test_get_existing_agent() {
        let reg = RegistryState::new();
        let a = make_agent("a1", vec![], "active");
        reg.agents.insert("a1".into(), a);
        let found = reg.get("a1").unwrap();
        assert_eq!(found.info.agent_id, "a1");
    }

    #[test]
    fn test_get_missing_agent() {
        let reg = RegistryState::new();
        assert!(reg.get("nope").is_none());
    }

    #[test]
    fn test_find_by_skill_filters_active_only() {
        let reg = RegistryState::new();
        reg.agents.insert("a1".into(), make_agent("a1", vec!["search".into()], "active"));
        reg.agents.insert("a2".into(), make_agent("a2", vec!["search".into()], "offline"));
        let results = reg.find_by_skill("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].info.agent_id, "a1");
    }

    #[test]
    fn test_find_by_skill_no_match() {
        let reg = RegistryState::new();
        reg.agents.insert("a1".into(), make_agent("a1", vec!["chat".into()], "active"));
        assert!(reg.find_by_skill("search").is_empty());
    }

    #[test]
    fn test_agent_info_default_port() {
        let info: AgentInfo = serde_json::from_str(r#"{"agent_id":"x","hostname":"h"}"#).unwrap();
        assert_eq!(info.port, 8000);
    }

    #[test]
    fn test_agent_capabilities_default() {
        let caps = AgentCapabilities::default();
        assert!(caps.skills.is_empty());
        assert!(caps.mcp_servers.is_empty());
        assert_eq!(caps.goals_count, 0);
        assert_eq!(caps.mode, "");
    }
}
