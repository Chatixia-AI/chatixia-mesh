//! Hub API — task queue and monitoring.

use axum::extract::{Path, State};
use axum::Json;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;
use uuid::Uuid;

use crate::AppState;

/// Task submission payload.
#[derive(Debug, Deserialize)]
pub struct TaskSubmission {
    #[serde(default)]
    pub skill: String,
    #[serde(default)]
    pub target_agent_id: String,
    #[serde(default)]
    pub source_agent_id: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default = "default_ttl")]
    pub ttl: u64,
}

fn default_ttl() -> u64 {
    300
}

/// Task update payload.
#[derive(Debug, Deserialize)]
pub struct TaskUpdate {
    pub state: String,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub error: String,
}

/// Task record.
#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub skill: String,
    pub target_agent_id: String,
    pub source_agent_id: String,
    pub assigned_agent_id: String,
    pub payload: serde_json::Value,
    pub state: String,
    pub result: String,
    pub error: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub ttl: u64,
}

pub struct HubState {
    tasks: DashMap<String, Task>,
}

impl HubState {
    pub fn new() -> Self {
        Self {
            tasks: DashMap::new(),
        }
    }

    /// Background loop to expire stale tasks.
    pub async fn expire_tasks_loop(&self) {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let now = epoch_now();
            for mut entry in self.tasks.iter_mut() {
                let task = entry.value_mut();
                if task.state == "completed" || task.state == "failed" {
                    continue;
                }
                if now - task.created_at > task.ttl as f64 {
                    task.state = "failed".into();
                    task.error = "TTL expired".into();
                    task.updated_at = now;
                }
            }
        }
    }

    /// Find pending tasks matching an agent's skills.
    pub fn get_pending_for_agent(&self, agent_id: &str, skills: &[String]) -> Vec<Task> {
        let now = epoch_now();
        let mut result = Vec::new();
        for mut entry in self.tasks.iter_mut() {
            let task = entry.value_mut();
            if task.state != "pending" {
                continue;
            }
            let matches = task.target_agent_id == agent_id
                || (!task.skill.is_empty() && skills.contains(&task.skill));
            if matches {
                task.state = "assigned".into();
                task.assigned_agent_id = agent_id.to_string();
                task.updated_at = now;
                result.push(task.clone());
            }
        }
        result
    }
}

fn epoch_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

// ─── HTTP Handlers ───────────────────────────────────────────────────────

/// POST /api/hub/tasks — submit a task.
pub async fn submit_task(
    State(state): State<AppState>,
    Json(sub): Json<TaskSubmission>,
) -> Json<serde_json::Value> {
    let task_id = Uuid::new_v4().to_string()[..12].to_string();
    let now = epoch_now();

    let task = Task {
        id: task_id.clone(),
        skill: sub.skill,
        target_agent_id: sub.target_agent_id,
        source_agent_id: sub.source_agent_id,
        assigned_agent_id: String::new(),
        payload: sub.payload,
        state: "pending".into(),
        result: String::new(),
        error: String::new(),
        created_at: now,
        updated_at: now,
        ttl: sub.ttl,
    };

    info!("[HUB] task submitted: {}", task_id);
    state.hub.tasks.insert(task_id.clone(), task);

    Json(serde_json::json!({ "task_id": task_id }))
}

/// GET /api/hub/tasks/all — list all tasks.
pub async fn list_tasks(State(state): State<AppState>) -> Json<Vec<Task>> {
    let tasks: Vec<Task> = state.hub.tasks.iter().map(|e| e.value().clone()).collect();
    Json(tasks)
}

/// GET /api/hub/tasks/:task_id — get task status.
pub async fn get_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Json<serde_json::Value> {
    match state.hub.tasks.get(&task_id) {
        Some(task) => Json(serde_json::to_value(task.value()).unwrap()),
        None => Json(serde_json::json!({ "error": "not found" })),
    }
}

/// POST /api/hub/tasks/:task_id — update task state/result.
pub async fn update_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(update): Json<TaskUpdate>,
) -> Json<serde_json::Value> {
    if let Some(mut entry) = state.hub.tasks.get_mut(&task_id) {
        let task = entry.value_mut();
        task.state = update.state;
        if !update.result.is_empty() {
            task.result = update.result;
        }
        if !update.error.is_empty() {
            task.error = update.error;
        }
        task.updated_at = epoch_now();
        Json(serde_json::json!({ "status": "ok" }))
    } else {
        Json(serde_json::json!({ "error": "not found" }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(id: &str, skill: &str, target: &str, state: &str) -> Task {
        let now = epoch_now();
        Task {
            id: id.to_string(),
            skill: skill.to_string(),
            target_agent_id: target.to_string(),
            source_agent_id: "src".into(),
            assigned_agent_id: String::new(),
            payload: serde_json::json!({}),
            state: state.to_string(),
            result: String::new(),
            error: String::new(),
            created_at: now,
            updated_at: now,
            ttl: 300,
        }
    }

    #[test]
    fn test_new_hub_is_empty() {
        let hub = HubState::new();
        let tasks: Vec<Task> = hub.tasks.iter().map(|e| e.value().clone()).collect();
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_submit_and_get_task() {
        let hub = HubState::new();
        let task = make_task("t1", "search", "a1", "pending");
        hub.tasks.insert("t1".into(), task);
        assert!(hub.tasks.get("t1").is_some());
        assert_eq!(hub.tasks.get("t1").unwrap().skill, "search");
    }

    #[test]
    fn test_get_pending_by_agent_id() {
        let hub = HubState::new();
        hub.tasks
            .insert("t1".into(), make_task("t1", "", "agent-1", "pending"));
        let result = hub.get_pending_for_agent("agent-1", &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "t1");
    }

    #[test]
    fn test_get_pending_by_skill() {
        let hub = HubState::new();
        hub.tasks
            .insert("t1".into(), make_task("t1", "search", "", "pending"));
        let result = hub.get_pending_for_agent("any-agent", &["search".to_string()]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "t1");
    }

    #[test]
    fn test_get_pending_marks_assigned() {
        let hub = HubState::new();
        hub.tasks
            .insert("t1".into(), make_task("t1", "search", "", "pending"));
        let result = hub.get_pending_for_agent("a1", &["search".to_string()]);
        assert_eq!(result[0].state, "assigned");
        // Verify it's also updated in the map
        assert_eq!(hub.tasks.get("t1").unwrap().state, "assigned");
    }

    #[test]
    fn test_get_pending_skips_completed() {
        let hub = HubState::new();
        hub.tasks
            .insert("t1".into(), make_task("t1", "search", "a1", "completed"));
        let result = hub.get_pending_for_agent("a1", &["search".to_string()]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_default_ttl() {
        assert_eq!(default_ttl(), 300);
    }

    #[test]
    fn test_task_serialization() {
        let task = make_task("t1", "search", "a1", "pending");
        let json = serde_json::to_value(&task).unwrap();
        assert_eq!(json["id"], "t1");
        assert_eq!(json["skill"], "search");
        assert_eq!(json["state"], "pending");
        assert_eq!(json["target_agent_id"], "a1");
    }
}
