import type { Agent } from '../api'
import { color, font, spacing, glass, radius, shadow } from '../theme'

const healthColors: Record<string, string> = {
  active: color.active,
  stale: color.stale,
  offline: color.offline,
}

const healthLabels: Record<string, string> = {
  active: 'Online',
  stale: 'Stale',
  offline: 'Offline',
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
        <div style={{
          textAlign: 'center',
          padding: spacing[12],
          color: color.onSurfaceMuted,
          fontFamily: font.body,
          fontSize: '0.875rem',
        }}>
          Waiting for agent heartbeats...
        </div>
      </Section>
    )
  }

  return (
    <Section title="connected agents" count={agents.length}>
      <div style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(auto-fill, minmax(340px, 1fr))',
        gap: spacing[4],
      }}>
        {agents.map(a => {
          const isSelected = selectedId === a.agent_id
          const hc = healthColors[a.health] || color.offline

          return (
            <div
              key={a.agent_id}
              onClick={() => onSelect(a.agent_id)}
              style={{
                ...glass.card,
                borderRadius: radius.lg,
                padding: spacing[5],
                cursor: 'pointer',
                transition: 'all 0.2s ease',
                border: `1px solid ${isSelected ? 'rgba(0,100,123,0.2)' : color.outlineVariant}`,
                boxShadow: isSelected ? shadow.float : shadow.ambient,
                ...(isSelected ? { background: 'rgba(0,207,252,0.06)' } : {}),
              }}
            >
              {/* Top row: status + name */}
              <div style={{
                display: 'flex',
                alignItems: 'center',
                gap: 10,
                marginBottom: spacing[3],
              }}>
                <div style={{
                  width: 10,
                  height: 10,
                  borderRadius: '50%',
                  background: hc,
                  boxShadow: `0 0 8px ${hc}`,
                  flexShrink: 0,
                }} />
                <span style={{
                  fontFamily: font.mono,
                  fontWeight: 600,
                  fontSize: '0.85rem',
                  color: color.onSurface,
                }}>{a.agent_id}</span>
                <span style={{
                  marginLeft: 'auto',
                  fontSize: '0.65rem',
                  fontWeight: 600,
                  fontFamily: font.display,
                  textTransform: 'uppercase',
                  letterSpacing: '0.05em',
                  color: hc,
                }}>{healthLabels[a.health] || a.health}</span>
              </div>

              {/* Detail rows */}
              <div style={{
                display: 'grid',
                gap: spacing[1],
                fontSize: '0.78rem',
                fontFamily: font.body,
                color: color.onSurfaceMuted,
              }}>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>host</span>
                  <span style={{ color: color.onSurface, fontFamily: font.mono, fontSize: '0.72rem' }}>{a.hostname || '—'}</span>
                </div>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>endpoint</span>
                  <span style={{ color: color.onSurface, fontFamily: font.mono, fontSize: '0.72rem' }}>{a.ip}:{a.port}</span>
                </div>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>peer</span>
                  <span style={{ color: color.onSurface, fontFamily: font.mono, fontSize: '0.72rem' }}>{a.sidecar_peer_id || 'no sidecar'}</span>
                </div>
                <div style={{
                  display: 'flex',
                  gap: 8,
                  marginTop: spacing[2],
                  flexWrap: 'wrap',
                }}>
                  <Pill label={`${a.capabilities?.skills?.length ?? 0} skills`} />
                  {a.mode && <Pill label={a.mode} />}
                </div>
              </div>
            </div>
          )
        })}
      </div>
    </Section>
  )
}

function Pill({ label }: { label: string }) {
  return (
    <span style={{
      fontSize: '0.65rem',
      fontWeight: 600,
      fontFamily: font.display,
      textTransform: 'uppercase',
      letterSpacing: '0.04em',
      padding: '3px 10px',
      borderRadius: radius.md,
      background: color.surfaceContainerLow,
      color: color.onSurfaceMuted,
    }}>{label}</span>
  )
}

function Section({ title, count, children }: { title: string; count: number; children: React.ReactNode }) {
  return (
    <div>
      <div style={{
        display: 'flex',
        alignItems: 'baseline',
        gap: 12,
        marginBottom: spacing[4],
      }}>
        <h2 style={{
          fontSize: '1.6rem',
          fontWeight: 700,
          fontFamily: font.display,
          letterSpacing: '-0.02em',
          color: color.onSurface,
        }}><span style={{ color: color.onSurfaceMuted, fontWeight: 400 }}>// </span>{title}</h2>
        <span style={{
          fontSize: '0.75rem',
          fontWeight: 500,
          color: color.onSurfaceMuted,
        }}>{count}</span>
      </div>
      {children}
    </div>
  )
}
