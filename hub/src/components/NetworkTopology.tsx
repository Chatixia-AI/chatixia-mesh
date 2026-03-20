import { useEffect, useRef } from 'react'
import type { Topology } from '../api'

const healthColors: Record<string, string> = {
  active: '#4ade80',
  stale: '#fbbf24',
  offline: '#f87171',
}

export function NetworkTopology({ topology }: { topology: Topology | null }) {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || !topology) return

    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const rect = canvas.parentElement!.getBoundingClientRect()
    const W = rect.width
    const H = 400

    canvas.width = W * dpr
    canvas.height = H * dpr
    canvas.style.width = `${W}px`
    canvas.style.height = `${H}px`
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)

    // Clear
    ctx.fillStyle = '#0d1117'
    ctx.fillRect(0, 0, W, H)

    const nodes = topology.nodes || []
    if (!nodes.length) {
      ctx.fillStyle = '#4a5568'
      ctx.font = '12px JetBrains Mono, monospace'
      ctx.textAlign = 'center'
      ctx.fillText('waiting for agents to join the mesh...', W / 2, H / 2)
      return
    }

    // Layout: hub in center, agents in circle
    const hubX = W / 2
    const hubY = H / 2
    const radius = Math.min(W, H) * 0.35

    // Draw hub
    ctx.shadowColor = '#60a5fa'
    ctx.shadowBlur = 14
    ctx.fillStyle = '#60a5fa'
    ctx.beginPath()
    ctx.arc(hubX, hubY, 18, 0, Math.PI * 2)
    ctx.fill()
    ctx.shadowBlur = 0
    ctx.fillStyle = '#e6edf3'
    ctx.font = 'bold 9px JetBrains Mono, monospace'
    ctx.textAlign = 'center'
    ctx.fillText('REGISTRY', hubX, hubY + 32)

    // Position nodes
    const positions = nodes.map((_, i) => {
      const angle = (2 * Math.PI * i) / nodes.length - Math.PI / 2
      return { x: hubX + Math.cos(angle) * radius, y: hubY + Math.sin(angle) * radius }
    })

    // Draw edges (hub → agent)
    nodes.forEach((_, i) => {
      ctx.strokeStyle = 'rgba(30,42,58,.5)'
      ctx.lineWidth = 1
      ctx.beginPath()
      ctx.moveTo(hubX, hubY)
      ctx.lineTo(positions[i].x, positions[i].y)
      ctx.stroke()
    })

    // Draw mesh edges (agent ↔ agent)
    for (const edge of topology.mesh_edges || []) {
      const fromIdx = nodes.findIndex(n => n.sidecar_peer_id === edge.from_peer)
      const toIdx = nodes.findIndex(n => n.sidecar_peer_id === edge.to_peer)
      if (fromIdx >= 0 && toIdx >= 0) {
        ctx.strokeStyle = 'rgba(96,165,250,.2)'
        ctx.setLineDash([3, 4])
        ctx.beginPath()
        ctx.moveTo(positions[fromIdx].x, positions[fromIdx].y)
        ctx.lineTo(positions[toIdx].x, positions[toIdx].y)
        ctx.stroke()
        ctx.setLineDash([])
      }
    }

    // Draw agent nodes
    nodes.forEach((node, i) => {
      const { x, y } = positions[i]
      const color = healthColors[node.health] || '#f87171'

      ctx.shadowColor = color
      ctx.shadowBlur = 8
      ctx.fillStyle = color
      ctx.beginPath()
      ctx.arc(x, y, 10, 0, Math.PI * 2)
      ctx.fill()
      ctx.shadowBlur = 0

      // Label
      ctx.fillStyle = '#e6edf3'
      ctx.font = 'bold 9px JetBrains Mono, monospace'
      ctx.textAlign = 'center'
      const label = node.agent_id.length > 14 ? node.agent_id.slice(0, 12) + '..' : node.agent_id
      ctx.fillText(label, x, y + 22)

      ctx.fillStyle = '#6b7d93'
      ctx.font = '8px JetBrains Mono, monospace'
      ctx.fillText(`${node.skills_count} skills`, x, y + 32)
    })

  }, [topology])

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
        <h2 style={{ fontSize: 12, fontWeight: 600, textTransform: 'uppercase', letterSpacing: '.06em', color: '#6b7d93' }}>mesh topology</h2>
        <span style={{ fontSize: 11, color: '#4a5568', background: '#131921', border: '1px solid #1e2a3a', padding: '1px 6px', borderRadius: 3 }}>
          {topology?.nodes?.length ?? 0} nodes
        </span>
      </div>
      <div style={{ border: '1px solid #1e2a3a', borderRadius: 4, background: '#0d1117', overflow: 'hidden', position: 'relative' }}>
        <canvas ref={canvasRef} style={{ width: '100%', height: 400, display: 'block' }} />
        <div style={{
          position: 'absolute', bottom: 10, right: 14,
          display: 'flex', gap: 14, fontSize: 10, color: '#4a5568',
          background: 'rgba(10,14,20,.85)', padding: '4px 10px', borderRadius: 3, border: '1px solid #1e2a3a',
        }}>
          <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}><span style={{ width: 8, height: 8, borderRadius: '50%', background: '#4ade80', display: 'inline-block' }} /> active</span>
          <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}><span style={{ width: 8, height: 8, borderRadius: '50%', background: '#fbbf24', display: 'inline-block' }} /> stale</span>
          <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}><span style={{ width: 8, height: 8, borderRadius: '50%', background: '#60a5fa', display: 'inline-block' }} /> registry</span>
          <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>--- mesh link</span>
        </div>
      </div>
    </div>
  )
}
