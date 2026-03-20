import type { Task } from '../api'

const stateColors: Record<string, string> = {
  pending: '#fbbf24',
  assigned: '#60a5fa',
  completed: '#4ade80',
  failed: '#f87171',
}

export function TaskQueue({ tasks }: { tasks: Task[] }) {
  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
        <h2 style={{ fontSize: 12, fontWeight: 600, textTransform: 'uppercase', letterSpacing: '.06em', color: '#6b7d93' }}>task queue</h2>
        <span style={{ fontSize: 11, color: '#4a5568', background: '#131921', border: '1px solid #1e2a3a', padding: '1px 6px', borderRadius: 3 }}>{tasks.length}</span>
      </div>
      {!tasks.length ? (
        <div style={{ textAlign: 'center', padding: 24, color: '#4a5568', fontSize: 12 }}>no tasks</div>
      ) : (
        <table style={{ width: '100%', borderCollapse: 'collapse', border: '1px solid #1e2a3a', borderRadius: 4 }}>
          <thead>
            <tr style={{ background: '#131921' }}>
              {['id', 'state', 'skill', 'source', 'target', 'age'].map(h => (
                <th key={h} style={{ padding: '8px 14px', textAlign: 'left', fontSize: 10, fontWeight: 600, textTransform: 'uppercase', letterSpacing: '.06em', color: '#4a5568', borderBottom: '1px solid #1e2a3a' }}>{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {tasks.map(t => (
              <tr key={t.id} style={{ background: '#0d1117', borderBottom: '1px solid #1e2a3a' }}>
                <td style={{ padding: '8px 14px', fontSize: 12, color: '#6b7d93' }}>{t.id}</td>
                <td style={{ padding: '8px 14px' }}>
                  <span style={{
                    fontSize: 10, fontWeight: 600, textTransform: 'uppercase',
                    padding: '2px 6px', borderRadius: 3,
                    color: stateColors[t.state] || '#6b7d93',
                    background: `${stateColors[t.state] || '#6b7d93'}15`,
                  }}>{t.state}</span>
                </td>
                <td style={{ padding: '8px 14px', fontSize: 12 }}>{t.skill || '—'}</td>
                <td style={{ padding: '8px 14px', fontSize: 12, color: '#6b7d93' }}>{t.source_agent_id || '—'}</td>
                <td style={{ padding: '8px 14px', fontSize: 12, color: '#6b7d93' }}>{t.target_agent_id || t.assigned_agent_id || '—'}</td>
                <td style={{ padding: '8px 14px', fontSize: 12, color: '#6b7d93' }}>{formatAge(t.created_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}

function formatAge(epoch: number): string {
  const seconds = Math.floor(Date.now() / 1000 - epoch)
  if (seconds < 60) return `${seconds}s`
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`
  return `${Math.floor(seconds / 3600)}h`
}
