import { useEffect, useState } from 'react'
import { Agent, Task, Topology, fetchAgents, fetchTasks, fetchTopology, submitTask } from './api'
import { AgentCards } from './components/AgentCards'
import { TaskQueue } from './components/TaskQueue'
import { NetworkTopology } from './components/NetworkTopology'
import { AgentChat } from './components/AgentChat'

export default function App() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [tasks, setTasks] = useState<Task[]>([])
  const [topology, setTopology] = useState<Topology | null>(null)
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null)

  useEffect(() => {
    const refresh = async () => {
      try {
        const [a, t, topo] = await Promise.all([
          fetchAgents(),
          fetchTasks(),
          fetchTopology(),
        ])
        setAgents(Array.isArray(a) ? a : [])
        setTasks(Array.isArray(t) ? t : [])
        setTopology(topo)
      } catch (e) {
        console.error('refresh error:', e)
      }
    }

    refresh()
    const interval = setInterval(refresh, 5000)
    return () => clearInterval(interval)
  }, [])

  const activeCount = agents.filter(a => a.health === 'active').length
  const pendingCount = tasks.filter(t => t.state === 'pending').length

  return (
    <div style={{ fontFamily: "'JetBrains Mono', monospace", background: '#0a0e14', color: '#c5cdd8', minHeight: '100vh' }}>
      {/* Header */}
      <header style={{ display: 'flex', alignItems: 'center', gap: 16, padding: '12px 24px', borderBottom: '1px solid #1e2a3a', background: '#0d1117' }}>
        <div style={{ border: '1px solid #22c55e', color: '#4ade80', width: 28, height: 28, display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 11, fontWeight: 700, borderRadius: 3 }}>&gt;_</div>
        <h1 style={{ fontSize: 14, fontWeight: 600 }}>chatixia <span style={{ color: '#4a5568' }}>//</span> mesh hub</h1>
        <span style={{ color: '#6b7d93', fontSize: 12, marginLeft: 8 }}>agent network monitor</span>
        <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: '#6b7d93' }}>
          <div style={{ width: 6, height: 6, borderRadius: '50%', background: '#4ade80', animation: 'blink 2s ease-in-out infinite' }} />
          {new Date().toLocaleTimeString('en', { hour12: false })}
        </div>
      </header>

      {/* Stats */}
      <div style={{ display: 'flex', gap: 1, background: '#1e2a3a', borderBottom: '1px solid #1e2a3a' }}>
        <Stat label="agents online" value={activeCount} color="#4ade80" />
        <Stat label="total agents" value={agents.length} color="#60a5fa" />
        <Stat label="pending tasks" value={pendingCount} color="#fbbf24" />
        <Stat label="total tasks" value={tasks.length} color="#6b7d93" />
      </div>

      {/* Main content */}
      <div style={{ padding: '20px 24px', display: 'grid', gap: 24 }}>
        <AgentCards agents={agents} onSelect={setSelectedAgent} selectedId={selectedAgent} />

        {selectedAgent && (
          <AgentChat agentId={selectedAgent} onClose={() => setSelectedAgent(null)} />
        )}

        <NetworkTopology topology={topology} />
        <TaskQueue tasks={tasks} />
      </div>

      <style>{`@keyframes blink { 0%,100%{opacity:1} 50%{opacity:.3} }`}</style>
    </div>
  )
}

function Stat({ label, value, color }: { label: string; value: number; color: string }) {
  return (
    <div style={{ flex: 1, padding: '12px 20px', background: '#0d1117' }}>
      <div style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: '.08em', color: '#4a5568', marginBottom: 4 }}>{label}</div>
      <div style={{ fontSize: 20, fontWeight: 700, color }}>{value}</div>
    </div>
  )
}
