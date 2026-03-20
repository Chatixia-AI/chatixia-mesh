import { useCallback, useEffect, useState } from 'react'
import { Agent, Task, Topology, OnboardingEntry, fetchAgents, fetchTasks, fetchTopology, fetchPendingApprovals } from './api'
import { AgentCards } from './components/AgentCards'
import { TaskQueue } from './components/TaskQueue'
import { NetworkTopology } from './components/NetworkTopology'
import { AgentChat } from './components/AgentChat'
import { ApprovalQueue } from './components/ApprovalQueue'
import { color, font, spacing, glass, gradient, radius, shadow } from './theme'

export default function App() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [tasks, setTasks] = useState<Task[]>([])
  const [topology, setTopology] = useState<Topology | null>(null)
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null)
  const [pendingApprovals, setPendingApprovals] = useState<OnboardingEntry[]>([])
  const [clock, setClock] = useState(() => new Date().toLocaleTimeString('en', { hour12: false }))

  const refresh = useCallback(async () => {
    try {
      const [a, t, topo, pending] = await Promise.all([
        fetchAgents(),
        fetchTasks(),
        fetchTopology(),
        fetchPendingApprovals(),
      ])
      setAgents(Array.isArray(a) ? a : [])
      setTasks(Array.isArray(t) ? t : [])
      setTopology(topo)
      setPendingApprovals(Array.isArray(pending) ? pending : [])
    } catch (e) {
      console.error('refresh error:', e)
    }
  }, [])

  useEffect(() => {
    refresh()
    const interval = setInterval(refresh, 5000)
    const tick = setInterval(() => setClock(new Date().toLocaleTimeString('en', { hour12: false })), 1000)
    return () => { clearInterval(interval); clearInterval(tick) }
  }, [refresh])

  const activeCount = agents.filter(a => a.health === 'active').length
  const pendingCount = tasks.filter(t => t.state === 'pending').length

  return (
    <div style={{
      fontFamily: font.body,
      background: color.surface,
      color: color.onSurface,
      minHeight: '100vh',
    }}>
      {/* Glass Header */}
      <header style={{
        position: 'sticky',
        top: 0,
        zIndex: 100,
        display: 'flex',
        alignItems: 'center',
        gap: 16,
        padding: `${spacing[4]} ${spacing[12]}`,
        ...glass.header,
      }}>
        <div style={{
          background: gradient.primary,
          color: color.onPrimary,
          width: 36,
          height: 36,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          fontSize: 13,
          fontWeight: 700,
          fontFamily: font.mono,
          borderRadius: radius.sm,
        }}>&gt;_</div>
        <div>
          <h1 style={{
            fontSize: '1.1rem',
            fontWeight: 600,
            fontFamily: font.display,
            letterSpacing: '-0.02em',
            color: color.onSurface,
          }}>chatixia <span style={{ color: color.onSurfaceMuted, fontWeight: 400 }}>//</span> <span style={{ fontWeight: 400, color: color.onSurfaceMuted }}>mesh hub</span></h1>
        </div>
        <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 14 }}>
          <div style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            fontSize: '0.75rem',
            fontWeight: 500,
            color: color.onSurfaceMuted,
          }}>
            <div style={{
              width: 7,
              height: 7,
              borderRadius: '50%',
              background: color.active,
              boxShadow: `0 0 8px ${color.active}`,
              animation: 'pulse 2s ease-in-out infinite',
            }} />
            connected
          </div>
          <span style={{
            fontFamily: font.mono,
            fontSize: '0.72rem',
            fontWeight: 500,
            color: color.onSurfaceMuted,
            letterSpacing: '-0.02em',
          }}>{clock}</span>
        </div>
      </header>

      {/* Stats Row */}
      <div style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(4, 1fr)',
        gap: spacing[4],
        padding: `${spacing[6]} ${spacing[12]}`,
      }}>
        <StatCard label="agents online" value={activeCount} accent={color.active} />
        <StatCard label="total agents" value={agents.length} accent={color.primary} />
        <StatCard label="pending tasks" value={pendingCount} accent={color.stale} />
        <StatCard label="awaiting approval" value={pendingApprovals.length} accent={color.stale} />
      </div>

      {/* Main Content */}
      <div style={{
        padding: `0 ${spacing[12]} ${spacing[12]}`,
        display: 'grid',
        gap: spacing[8],
      }}>
        <ApprovalQueue entries={pendingApprovals} onAction={refresh} />

        <AgentCards agents={agents} onSelect={setSelectedAgent} selectedId={selectedAgent} />

        {selectedAgent && (
          <AgentChat agentId={selectedAgent} onClose={() => setSelectedAgent(null)} />
        )}

        <NetworkTopology topology={topology} />
        <TaskQueue tasks={tasks} />
      </div>

      <style>{`
        @keyframes pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.4; }
        }
      `}</style>
    </div>
  )
}

function StatCard({ label, value, accent }: { label: string; value: number; accent: string }) {
  return (
    <div style={{
      ...glass.card,
      borderRadius: radius.lg,
      padding: spacing[6],
      border: `1px solid ${color.outlineVariant}`,
      boxShadow: shadow.ambient,
      transition: 'transform 0.2s ease, box-shadow 0.2s ease',
    }}>
      <div style={{
        fontSize: '0.7rem',
        fontWeight: 600,
        fontFamily: font.display,
        textTransform: 'uppercase',
        letterSpacing: '0.06em',
        color: color.onSurfaceMuted,
        marginBottom: spacing[2],
      }}>{label}</div>
      <div style={{
        fontSize: '2.2rem',
        fontWeight: 700,
        fontFamily: font.mono,
        letterSpacing: '-0.04em',
        color: accent,
        lineHeight: 1,
      }}>{value}</div>
    </div>
  )
}
