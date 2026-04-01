//! Network topology — agent positions and RTT tracking for visualization.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct TopologyNode {
    pub agent_id: String,
    pub ip: String,
    pub port: u16,
    pub hostname: String,
    pub sidecar_peer_id: String,
    pub mode: String,
    pub skills_count: usize,
    pub health: String,
    /// Peer IDs this agent has active DataChannels with.
    pub mesh_peers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopologyResponse {
    pub nodes: Vec<TopologyNode>,
    pub mesh_edges: Vec<MeshEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeshEdge {
    pub from_peer: String,
    pub to_peer: String,
}

/// GET /api/hub/network/topology — return mesh topology for visualization.
pub async fn network_topology(State(state): State<AppState>) -> Json<TopologyResponse> {
    let agents = state.registry.list();

    let nodes: Vec<TopologyNode> = agents
        .iter()
        .map(|a| {
            // Build list of potential mesh peers (all other agents with sidecar peer IDs)
            let mesh_peers: Vec<String> = agents
                .iter()
                .filter(|other| {
                    other.info.agent_id != a.info.agent_id
                        && !other.info.sidecar_peer_id.is_empty()
                        && other.health == "active"
                })
                .map(|other| other.info.sidecar_peer_id.clone())
                .collect();

            TopologyNode {
                agent_id: a.info.agent_id.clone(),
                ip: a.info.ip.clone(),
                port: a.info.port,
                hostname: a.info.hostname.clone(),
                sidecar_peer_id: a.info.sidecar_peer_id.clone(),
                mode: a.info.mode.clone(),
                skills_count: a.info.capabilities.skills.len(),
                health: a.health.clone(),
                mesh_peers,
            }
        })
        .collect();

    // Build full-mesh edges between active agents with sidecars
    let mut edges = Vec::new();
    let active_peers: Vec<&TopologyNode> = nodes
        .iter()
        .filter(|n| n.health == "active" && !n.sidecar_peer_id.is_empty())
        .collect();

    for i in 0..active_peers.len() {
        for j in (i + 1)..active_peers.len() {
            edges.push(MeshEdge {
                from_peer: active_peers[i].sidecar_peer_id.clone(),
                to_peer: active_peers[j].sidecar_peer_id.clone(),
            });
        }
    }

    Json(TopologyResponse {
        nodes,
        mesh_edges: edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_node_serialization() {
        let node = TopologyNode {
            agent_id: "a1".into(),
            ip: "10.0.0.1".into(),
            port: 8000,
            hostname: "host1".into(),
            sidecar_peer_id: "sc-1".into(),
            mode: "auto".into(),
            skills_count: 3,
            health: "active".into(),
            mesh_peers: vec!["sc-2".into()],
        };
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["agent_id"], "a1");
        assert_eq!(json["port"], 8000);
        assert_eq!(json["skills_count"], 3);
        assert_eq!(json["mesh_peers"][0], "sc-2");
    }

    #[test]
    fn test_mesh_edge_serialization() {
        let edge = MeshEdge {
            from_peer: "sc-1".into(),
            to_peer: "sc-2".into(),
        };
        let json = serde_json::to_value(&edge).unwrap();
        assert_eq!(json["from_peer"], "sc-1");
        assert_eq!(json["to_peer"], "sc-2");
    }

    #[test]
    fn test_topology_response_empty() {
        let resp = TopologyResponse {
            nodes: vec![],
            mesh_edges: vec![],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["nodes"].as_array().unwrap().len(), 0);
        assert_eq!(json["mesh_edges"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_topology_response_with_multiple_nodes() {
        let resp = TopologyResponse {
            nodes: vec![
                TopologyNode {
                    agent_id: "a1".into(),
                    ip: "10.0.0.1".into(),
                    port: 8000,
                    hostname: "host1".into(),
                    sidecar_peer_id: "sc-1".into(),
                    mode: "auto".into(),
                    skills_count: 3,
                    health: "active".into(),
                    mesh_peers: vec!["sc-2".into()],
                },
                TopologyNode {
                    agent_id: "a2".into(),
                    ip: "10.0.0.2".into(),
                    port: 8000,
                    hostname: "host2".into(),
                    sidecar_peer_id: "sc-2".into(),
                    mode: "interactive".into(),
                    skills_count: 5,
                    health: "active".into(),
                    mesh_peers: vec!["sc-1".into()],
                },
            ],
            mesh_edges: vec![MeshEdge {
                from_peer: "sc-1".into(),
                to_peer: "sc-2".into(),
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(json["mesh_edges"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_topology_node_empty_mesh_peers() {
        let node = TopologyNode {
            agent_id: "solo".into(),
            ip: "127.0.0.1".into(),
            port: 8000,
            hostname: "localhost".into(),
            sidecar_peer_id: "sc-solo".into(),
            mode: "auto".into(),
            skills_count: 0,
            health: "active".into(),
            mesh_peers: vec![],
        };
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["mesh_peers"].as_array().unwrap().len(), 0);
        assert_eq!(json["skills_count"], 0);
    }
}
