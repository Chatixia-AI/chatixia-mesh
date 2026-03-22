import { useState } from 'react'
import type { Task } from '../api'
import { color, font, spacing, glass, radius, shadow } from '../theme'

const stateStyles: Record<string, { color: string; bg: string }> = {
  pending: { color: color.stale, bg: 'rgba(217,119,6,0.08)' },
  assigned: { color: color.info, bg: 'rgba(2,132,199,0.08)' },
  completed: { color: color.active, bg: 'rgba(22,163,74,0.08)' },
  failed: { color: color.offline, bg: 'rgba(220,38,38,0.08)' },
}

export function TaskQueue({ tasks }: { tasks: Task[] }) {
  const [expandedId, setExpandedId] = useState<string | null>(null)

  return (
    <div>
      <h2 style={{
        fontSize: '1.6rem',
        fontWeight: 700,
        fontFamily: font.display,
        letterSpacing: '-0.02em',
        color: color.onSurface,
        marginBottom: spacing[4],
      }}>
        <span style={{ color: color.onSurfaceMuted, fontWeight: 400 }}>// </span>task queue
        <span style={{
          fontSize: '0.75rem',
          fontWeight: 500,
          color: color.onSurfaceMuted,
          marginLeft: 12,
        }}>{tasks.length}</span>
      </h2>

      {!tasks.length ? (
        <div style={{
          ...glass.card,
          borderRadius: radius.lg,
          padding: spacing[12],
          textAlign: 'center',
          color: color.onSurfaceMuted,
          fontSize: '0.875rem',
          fontFamily: font.body,
          border: `1px solid ${color.outlineVariant}`,
          boxShadow: shadow.ambient,
        }}>
          No tasks in queue
        </div>
      ) : (
        <div style={{
          ...glass.card,
          borderRadius: radius.lg,
          border: `1px solid ${color.outlineVariant}`,
          boxShadow: shadow.ambient,
          overflow: 'hidden',
        }}>
          {/* Header row */}
          <div style={{
            display: 'grid',
            gridTemplateColumns: '1.5fr 0.8fr 1fr 1fr 1fr 0.6fr',
            padding: `${spacing[3]} ${spacing[5]}`,
            background: color.surfaceContainerLow,
          }}>
            {['ID', 'State', 'Skill', 'Source', 'Target', 'Age'].map(h => (
              <div key={h} style={{
                fontSize: '0.65rem',
                fontWeight: 700,
                fontFamily: font.display,
                textTransform: 'uppercase',
                letterSpacing: '0.06em',
                color: color.onSurfaceMuted,
              }}>{h}</div>
            ))}
          </div>

          {/* Task rows — click to expand details */}
          <div style={{ padding: `${spacing[2]} 0` }}>
            {tasks.map(t => {
              const style = stateStyles[t.state] || { color: color.onSurfaceMuted, bg: 'transparent' }
              const isExpanded = expandedId === t.id
              return (
                <div key={t.id}>
                  <div
                    style={{
                      display: 'grid',
                      gridTemplateColumns: '1.5fr 0.8fr 1fr 1fr 1fr 0.6fr',
                      padding: `${spacing[3]} ${spacing[5]}`,
                      alignItems: 'center',
                      cursor: 'pointer',
                      transition: 'background 0.15s ease',
                      background: isExpanded ? color.surfaceContainerLow : 'transparent',
                    }}
                    onClick={() => setExpandedId(isExpanded ? null : t.id)}
                    onMouseEnter={e => { if (!isExpanded) e.currentTarget.style.background = color.surfaceContainerLow }}
                    onMouseLeave={e => { if (!isExpanded) e.currentTarget.style.background = 'transparent' }}
                  >
                    <div style={{
                      fontSize: '0.73rem',
                      fontFamily: font.mono,
                      color: color.onSurfaceMuted,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                      paddingRight: spacing[3],
                    }}>{t.id}</div>

                    <div>
                      <span style={{
                        fontSize: '0.65rem',
                        fontWeight: 600,
                        fontFamily: font.display,
                        textTransform: 'uppercase',
                        letterSpacing: '0.04em',
                        padding: '3px 10px',
                        borderRadius: radius.md,
                        color: style.color,
                        background: style.bg,
                      }}>{t.state}</span>
                    </div>

                    <div style={{ fontSize: '0.73rem', fontFamily: font.mono, color: color.onSurface }}>
                      {t.skill || '—'}
                    </div>

                    <div style={{ fontSize: '0.73rem', fontFamily: font.mono, color: color.onSurfaceMuted }}>
                      {t.source_agent_id || '—'}
                    </div>

                    <div style={{ fontSize: '0.73rem', fontFamily: font.mono, color: color.onSurfaceMuted }}>
                      {t.target_agent_id || t.assigned_agent_id || '—'}
                    </div>

                    <div style={{ fontSize: '0.73rem', fontFamily: font.mono, color: color.onSurfaceMuted }}>
                      {formatAge(t.created_at)}
                    </div>
                  </div>

                  {isExpanded && <TaskDetail task={t} />}
                </div>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}

function TaskDetail({ task }: { task: Task }) {
  const labelStyle = {
    fontSize: '0.65rem',
    fontWeight: 700,
    fontFamily: font.display,
    textTransform: 'uppercase' as const,
    letterSpacing: '0.06em',
    color: color.onSurfaceMuted,
    minWidth: 80,
  }
  const valueStyle = {
    fontSize: '0.73rem',
    fontFamily: font.mono,
    color: color.onSurface,
    wordBreak: 'break-all' as const,
  }

  const rows: [string, string][] = [
    ['Error', task.error || '—'],
    ['Result', task.result || '—'],
    ['Payload', JSON.stringify(task.payload, null, 2)],
    ['Assigned', task.assigned_agent_id || '—'],
    ['Created', new Date(task.created_at * 1000).toLocaleString()],
    ['Updated', new Date(task.updated_at * 1000).toLocaleString()],
    ['TTL', `${task.ttl}s`],
  ]

  return (
    <div style={{
      background: color.surfaceContainerLow,
      padding: `${spacing[3]} ${spacing[5]} ${spacing[4]} ${spacing[5]}`,
      borderTop: `1px solid ${color.outlineVariant}`,
      borderBottom: `1px solid ${color.outlineVariant}`,
    }}>
      {rows.map(([label, value]) => (
        <div key={label} style={{
          display: 'flex',
          gap: spacing[3],
          padding: `${spacing[1]} 0`,
          alignItems: label === 'Payload' ? 'flex-start' : 'center',
        }}>
          <span style={labelStyle}>{label}</span>
          {label === 'Payload' ? (
            <pre style={{
              ...valueStyle,
              margin: 0,
              whiteSpace: 'pre-wrap',
              fontSize: '0.68rem',
              background: color.surfaceContainer,
              padding: `${spacing[1]} ${spacing[2]}`,
              borderRadius: radius.sm,
              flex: 1,
            }}>{value}</pre>
          ) : label === 'Error' && task.error ? (
            <span style={{ ...valueStyle, color: color.offline }}>{value}</span>
          ) : (
            <span style={valueStyle}>{value}</span>
          )}
        </div>
      ))}
    </div>
  )
}

function formatAge(epoch: number): string {
  const seconds = Math.floor(Date.now() / 1000 - epoch)
  if (seconds < 60) return `${seconds}s`
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`
  return `${Math.floor(seconds / 3600)}h`
}
