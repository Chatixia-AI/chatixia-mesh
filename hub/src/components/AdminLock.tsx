import { useState } from 'react'
import { color, font, radius } from '../theme'

interface Props {
  unlocked: boolean
  onUnlock: (token: string) => void
  onLock: () => void
}

/** Header control for the registry admin token (kept in sessionStorage by api.ts). */
export function AdminLock({ unlocked, onUnlock, onLock }: Props) {
  const [value, setValue] = useState('')

  const pill = {
    fontSize: '0.7rem',
    fontWeight: 600,
    fontFamily: font.display,
    textTransform: 'uppercase' as const,
    letterSpacing: '0.04em',
    border: 'none',
    borderRadius: radius.sm,
    padding: '6px 12px',
    cursor: 'pointer',
  }

  if (unlocked) {
    return (
      <button
        onClick={onLock}
        title="Forget the admin token for this tab"
        style={{ ...pill, background: color.surfaceContainerLow, color: color.primary }}
      >admin · lock</button>
    )
  }

  const submit = () => {
    if (!value.trim()) return
    onUnlock(value.trim())
    setValue('')
  }

  return (
    <form
      onSubmit={e => { e.preventDefault(); submit() }}
      style={{ display: 'flex', alignItems: 'center', gap: 6 }}
    >
      <input
        type="password"
        value={value}
        onChange={e => setValue(e.target.value)}
        placeholder="admin token"
        aria-label="Registry admin token"
        autoComplete="off"
        style={{
          fontFamily: font.mono,
          fontSize: '0.72rem',
          padding: '6px 10px',
          width: 180,
          border: `1px solid ${color.outlineVariant}`,
          borderRadius: radius.sm,
          background: color.surfaceContainerLowest,
          color: color.onSurface,
        }}
      />
      <button type="submit" style={{ ...pill, background: color.primary, color: color.onPrimary }}>
        unlock
      </button>
    </form>
  )
}
