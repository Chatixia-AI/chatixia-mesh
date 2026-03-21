import { useState } from 'react'
import type { OnboardingEntry } from '../api'
import { approveAgent, rejectAgent } from '../api'
import { color, font, spacing, glass, radius, shadow } from '../theme'

interface Props {
  entries: OnboardingEntry[]
  onAction: () => void
}

export function ApprovalQueue({ entries, onAction }: Props) {
  const [loading, setLoading] = useState<string | null>(null)

  const handle = async (id: string, action: 'approve' | 'reject') => {
    setLoading(id)
    try {
      if (action === 'approve') await approveAgent(id)
      else await rejectAgent(id)
      onAction()
    } catch (e) {
      console.error(`${action} failed:`, e)
    } finally {
      setLoading(null)
    }
  }

  const formatAge = (epoch: number) => {
    const s = Math.floor(Date.now() / 1000 - epoch)
    if (s < 60) return `${s}s ago`
    if (s < 3600) return `${Math.floor(s / 60)}m ago`
    return `${Math.floor(s / 3600)}h ago`
  }

  if (!entries.length) return null

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: spacing[4] }}>
        <h2 style={{
          fontSize: '0.75rem',
          fontWeight: 600,
          fontFamily: font.display,
          textTransform: 'uppercase',
          letterSpacing: '0.06em',
          color: color.onSurfaceMuted,
        }}>pending approvals</h2>
        <span style={{
          fontSize: '0.7rem',
          fontWeight: 600,
          fontFamily: font.mono,
          color: color.stale,
          background: 'rgba(217,119,6,0.10)',
          padding: '2px 8px',
          borderRadius: radius.sm,
        }}>{entries.length}</span>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))', gap: spacing[4] }}>
        {entries.map(e => (
          <div key={e.id} style={{
            ...glass.card,
            borderRadius: radius.lg,
            padding: spacing[5],
            border: `1px solid ${color.outlineVariant}`,
            boxShadow: shadow.ambient,
          }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
              <div style={{
                width: 8, height: 8, borderRadius: '50%',
                background: color.stale,
                boxShadow: `0 0 6px ${color.stale}`,
              }} />
              <span style={{ fontWeight: 600, fontSize: '0.85rem', fontFamily: font.display, color: color.onSurface }}>{e.agent_name}</span>
              <span style={{
                marginLeft: 'auto',
                fontSize: '0.65rem',
                fontWeight: 500,
                color: color.onSurfaceMuted,
                fontFamily: font.mono,
              }}>{formatAge(e.created_at)}</span>
            </div>

            <div style={{ fontSize: '0.75rem', color: color.onSurfaceMuted, display: 'grid', gap: 3, marginBottom: 12, fontFamily: font.mono }}>
              <div>id: {e.id}</div>
              <div>peer: {e.peer_id}</div>
            </div>

            <div style={{ display: 'flex', gap: 8 }}>
              <button
                disabled={loading === e.id}
                onClick={() => handle(e.id, 'approve')}
                style={{
                  flex: 1,
                  padding: '7px 0',
                  fontSize: '0.72rem',
                  fontWeight: 600,
                  fontFamily: font.display,
                  textTransform: 'uppercase',
                  letterSpacing: '0.04em',
                  border: 'none',
                  borderRadius: radius.sm,
                  cursor: loading === e.id ? 'wait' : 'pointer',
                  background: color.active,
                  color: '#fff',
                  opacity: loading === e.id ? 0.6 : 1,
                  transition: 'opacity 0.15s',
                }}
              >approve</button>
              <button
                disabled={loading === e.id}
                onClick={() => handle(e.id, 'reject')}
                style={{
                  flex: 1,
                  padding: '7px 0',
                  fontSize: '0.72rem',
                  fontWeight: 600,
                  fontFamily: font.display,
                  textTransform: 'uppercase',
                  letterSpacing: '0.04em',
                  border: `1px solid ${color.error}`,
                  borderRadius: radius.sm,
                  cursor: loading === e.id ? 'wait' : 'pointer',
                  background: color.errorContainer,
                  color: color.error,
                  opacity: loading === e.id ? 0.6 : 1,
                  transition: 'opacity 0.15s',
                }}
              >reject</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
