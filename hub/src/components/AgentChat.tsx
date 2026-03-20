import { useState } from 'react'
import { submitTask } from '../api'

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
        target_agent_id: agentId,
        source_agent_id: 'hub-user',
        payload: { message: message.trim(), type: 'user_intervention' },
      })
      setStatus(`task submitted: ${result.task_id}`)
      setMessage('')
    } catch (e) {
      setStatus(`error: ${e}`)
    }
  }

  return (
    <div style={{
      border: '1px solid #2a3a4e',
      borderRadius: 4,
      background: '#0d1117',
      padding: 16,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
        <h3 style={{ fontSize: 12, fontWeight: 600, color: '#60a5fa' }}>
          intervene: {agentId}
        </h3>
        <button
          onClick={onClose}
          style={{
            marginLeft: 'auto', background: 'none', border: '1px solid #1e2a3a',
            color: '#6b7d93', padding: '2px 8px', borderRadius: 3, cursor: 'pointer',
            fontFamily: 'inherit', fontSize: 11,
          }}
        >close</button>
      </div>
      <div style={{ display: 'flex', gap: 8 }}>
        <input
          type="text"
          value={message}
          onChange={e => setMessage(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && send()}
          placeholder="Send a task or message to this agent..."
          style={{
            flex: 1, background: '#131921', border: '1px solid #1e2a3a',
            color: '#c5cdd8', padding: '8px 12px', borderRadius: 4,
            fontFamily: 'inherit', fontSize: 12, outline: 'none',
          }}
        />
        <button
          onClick={send}
          style={{
            background: '#22c55e15', border: '1px solid #22c55e30',
            color: '#4ade80', padding: '8px 16px', borderRadius: 4,
            fontFamily: 'inherit', fontSize: 12, fontWeight: 600, cursor: 'pointer',
          }}
        >send</button>
      </div>
      {status && (
        <div style={{ marginTop: 8, fontSize: 11, color: '#6b7d93' }}>{status}</div>
      )}
    </div>
  )
}
