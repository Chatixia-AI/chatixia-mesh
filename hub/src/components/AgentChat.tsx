import { useState } from 'react'
import { submitTask } from '../api'
import { color, font, spacing, glass, radius, shadow, gradient } from '../theme'

interface Props {
  agentId: string
  onClose: () => void
}

export function AgentChat({ agentId, onClose }: Props) {
  const [message, setMessage] = useState('')
  const [status, setStatus] = useState<string | null>(null)

  const send = async () => {
    if (!message.trim()) return
    setStatus('sending...')
    try {
      const result = await submitTask({
        skill: 'user_intervention',
        target_agent_id: agentId,
        source_agent_id: 'hub-user',
        payload: { message: message.trim() },
      })
      setStatus(`Task submitted: ${result.task_id}`)
      setMessage('')
    } catch (e) {
      setStatus(`Error: ${e}`)
    }
  }

  return (
    <div style={{
      ...glass.card,
      borderRadius: radius.lg,
      padding: spacing[6],
      border: `1px solid ${color.outlineVariant}`,
      boxShadow: shadow.ambient,
    }}>
      <div style={{
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        marginBottom: spacing[5],
      }}>
        <h3 style={{
          fontSize: '1rem',
          fontWeight: 600,
          fontFamily: font.display,
          color: color.primary,
        }}>
          intervene <span style={{ color: color.onSurfaceMuted, fontWeight: 400 }}>//</span> <span style={{ fontFamily: font.mono, fontSize: '0.85rem' }}>{agentId}</span>
        </h3>
        <button
          onClick={onClose}
          style={{
            marginLeft: 'auto',
            background: color.surfaceContainerLow,
            border: 'none',
            color: color.onSurfaceMuted,
            padding: '6px 14px',
            borderRadius: radius.sm,
            cursor: 'pointer',
            fontFamily: font.display,
            fontSize: '0.75rem',
            fontWeight: 600,
            letterSpacing: '0.02em',
            transition: 'background 0.15s ease',
          }}
        >Close</button>
      </div>

      <div style={{ display: 'flex', gap: spacing[3] }}>
        <input
          type="text"
          value={message}
          onChange={e => setMessage(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && send()}
          placeholder="Send a task or message to this agent..."
          style={{
            flex: 1,
            background: color.surfaceContainerLow,
            border: 'none',
            borderBottom: '2px solid transparent',
            color: color.onSurface,
            padding: `${spacing[3]} ${spacing[4]}`,
            borderRadius: radius.sm,
            fontFamily: font.body,
            fontSize: '0.875rem',
            outline: 'none',
            transition: 'border-color 0.15s ease, background 0.15s ease',
          }}
          onFocus={e => {
            e.currentTarget.style.background = color.surfaceContainerLowest
            e.currentTarget.style.borderBottomColor = color.primaryContainer
          }}
          onBlur={e => {
            e.currentTarget.style.background = color.surfaceContainerLow
            e.currentTarget.style.borderBottomColor = 'transparent'
          }}
        />
        <button
          onClick={send}
          style={{
            background: gradient.primary,
            border: 'none',
            color: color.onPrimary,
            padding: `${spacing[3]} ${spacing[5]}`,
            borderRadius: radius.md,
            fontFamily: font.display,
            fontSize: '0.8rem',
            fontWeight: 600,
            letterSpacing: '0.02em',
            cursor: 'pointer',
            boxShadow: shadow.primaryGlow,
            transition: 'transform 0.15s ease, box-shadow 0.15s ease',
          }}
        >Send</button>
      </div>

      {status && (
        <div style={{
          marginTop: spacing[3],
          fontSize: '0.72rem',
          fontFamily: font.mono,
          color: color.onSurfaceMuted,
        }}>{status}</div>
      )}
    </div>
  )
}
