import type { Agent } from '../api'

const healthColors: Record<string, string> = {
  active: '#4ade80',
  stale: '#fbbf24',
  offline: '#f87171',
}

interface Props {
  agents: Agent[]
  onSelect: (id: string) => void
  selectedId: string | null
}

export function AgentCards({ agents, onSelect, selectedId }: Props) {
  if (!agents.length) {
    return (
      <Section title="connected agents" count={0}>
        <div style={{ textAlign: 'center', padding: 40, color: '#4a5568' }}>
          waiting for agent heartbeats...
        </div>
      </Section>
    )
  }

  return (
    <Section title="connected agents" count={agents.length}>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))', gap: 12 }}>
        {agents.map(a => (
          <div
            key={a.agent_id}
            onClick={() => onSelect(a.agent_id)}
            style={{
              background: selectedId === a.agent_id ? '#1a2233' : '#0d1117',
              border: `1px solid ${selectedId === a.agent_id ? '#2a3a4e' : '#1e2a3a'}`,
              borderRadius: 4,
              padding: '12px 16px',
              cursor: 'pointer',
              transition: 'all .15s',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
              <div style={{
                width: 8, height: 8, borderRadius: '50%',
                background: healthColors[a.health] || '#f87171',
                boxShadow: `0 0 6px ${healthColors[a.health] || '#f87171'}`,
              }} />
              <span style={{ fontWeight: 600, fontSize: 13 }}>{a.agent_id}</span>
              <span style={{
                marginLeft: 'auto',
                fontSize: 10, fontWeight: 600, textTransform: 'uppercase',
                color: healthColors[a.health],
              }}>{a.health}</span>
            </div>
            <div style={{ fontSize: 11, color: '#6b7d93', display: 'grid', gap: 2 }}>
              <div>host: {a.hostname || '—'} ({a.ip}:{a.port})</div>
              <div>peer: {a.sidecar_peer_id || 'no sidecar'}</div>
              <div>skills: {a.capabilities?.skills?.length ?? 0} | mode: {a.mode || '—'}</div>
            </div>
          </div>
        ))}
      </div>
    </Section>
  )
}

function Section({ title, count, children }: { title: string; count: number; children: React.ReactNode }) {
  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
        <h2 style={{ fontSize: 12, fontWeight: 600, textTransform: 'uppercase', letterSpacing: '.06em', color: '#6b7d93' }}>{title}</h2>
        <span style={{ fontSize: 11, color: '#4a5568', background: '#131921', border: '1px solid #1e2a3a', padding: '1px 6px', borderRadius: 3 }}>{count}</span>
      </div>
      {children}
    </div>
  )
}
