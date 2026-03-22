import { useEffect, useRef } from 'react'
import type { Topology } from '../api'
import { color, font, spacing, glass, radius, shadow } from '../theme'

const healthColors: Record<string, string> = {
  active: color.active,
  stale: color.stale,
  offline: color.offline,
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
    const H = 420

    canvas.width = W * dpr
    canvas.height = H * dpr
    canvas.style.width = `${W}px`
    canvas.style.height = `${H}px`
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)

    // Clear — light surface
    ctx.fillStyle = color.surfaceContainerLow
    ctx.fillRect(0, 0, W, H)

    const nodes = topology.nodes || []
    if (!nodes.length) {
      ctx.fillStyle = color.onSurfaceMuted
      ctx.font = `500 12px 'JetBrains Mono', monospace`
      ctx.textAlign = 'center'
      ctx.fillText('Waiting for agents to join the mesh...', W / 2, H / 2)
      return
    }

    // Layout: hub position + agent positions depend on node count
    const smallLayout = nodes.length <= 4
    const hubX = W / 2
    const hubY = smallLayout ? H * 0.30 : H / 2
    const circleRadius = Math.min(W, H) * 0.34

    // Position nodes
    const positions = nodes.map((_, i) => {
      if (smallLayout) {
        // Spread agents horizontally on a lower band
        const n = nodes.length
        const xStart = W * 0.30
        const xEnd = W * 0.70
        const x = n === 1 ? W / 2 : xStart + (xEnd - xStart) * (i / (n - 1))
        return { x, y: H * 0.70 }
      }
      // 5+ nodes: circular layout
      const angle = (2 * Math.PI * i) / nodes.length - Math.PI / 2
      return { x: hubX + Math.cos(angle) * circleRadius, y: hubY + Math.sin(angle) * circleRadius }
    })

    // Draw edges (hub → agent) — soft tonal lines
    nodes.forEach((_, i) => {
      ctx.strokeStyle = color.surfaceContainer
      ctx.lineWidth = 1.5
      ctx.beginPath()
      ctx.moveTo(hubX, hubY)
      ctx.lineTo(positions[i].x, positions[i].y)
      ctx.stroke()
    })

    // Draw hub node — gradient circle
    const hubGrad = ctx.createLinearGradient(hubX - 20, hubY - 20, hubX + 20, hubY + 20)
    hubGrad.addColorStop(0, '#00647b')
    hubGrad.addColorStop(1, '#00cffc')
    ctx.shadowColor = 'rgba(0,207,252,0.25)'
    ctx.shadowBlur = 20
    ctx.fillStyle = hubGrad
    ctx.beginPath()
    ctx.arc(hubX, hubY, 20, 0, Math.PI * 2)
    ctx.fill()
    ctx.shadowBlur = 0

    // Hub label
    ctx.fillStyle = color.onSurface
    ctx.font = `700 9px 'JetBrains Mono', monospace`
    ctx.textAlign = 'center'
    ctx.fillText('REGISTRY', hubX, hubY + 36)

    // Draw agent nodes
    nodes.forEach((node, i) => {
      const { x, y } = positions[i]
      const hc = healthColors[node.health] || color.offline

      // Ambient glow
      ctx.shadowColor = hc
      ctx.shadowBlur = 12
      // White circle base
      ctx.fillStyle = '#ffffff'
      ctx.beginPath()
      ctx.arc(x, y, 13, 0, Math.PI * 2)
      ctx.fill()
      ctx.shadowBlur = 0

      // Health dot inside
      ctx.fillStyle = hc
      ctx.beginPath()
      ctx.arc(x, y, 7, 0, Math.PI * 2)
      ctx.fill()

      // Label
      ctx.fillStyle = color.onSurface
      ctx.font = `600 9px 'JetBrains Mono', monospace`
      ctx.textAlign = 'center'
      const label = node.agent_id.length > 16 ? node.agent_id.slice(0, 14) + '..' : node.agent_id
      ctx.fillText(label, x, y + 26)

      // Sub-label
      ctx.fillStyle = color.onSurfaceMuted
      ctx.font = `500 8px 'JetBrains Mono', monospace`
      ctx.fillText(`${node.skills_count} skills`, x, y + 37)
    })

    // Draw mesh edges (agent <-> agent) — primary tinted dashes, drawn last so they appear on top
    for (const edge of topology.mesh_edges || []) {
      const fromIdx = nodes.findIndex(n => n.sidecar_peer_id === edge.from_peer)
      const toIdx = nodes.findIndex(n => n.sidecar_peer_id === edge.to_peer)
      if (fromIdx >= 0 && toIdx >= 0) {
        const from = positions[fromIdx]
        const to = positions[toIdx]
        // Compute a control point offset perpendicular to the edge, bowing toward the hub
        const mx = (from.x + to.x) / 2
        const my = (from.y + to.y) / 2
        const dx = to.x - from.x
        const dy = to.y - from.y
        const len = Math.sqrt(dx * dx + dy * dy) || 1
        // Perpendicular unit vector
        const px = -dy / len
        const py = dx / len
        // Offset toward the hub: pick the perpendicular direction closer to the hub
        const toHubX = hubX - mx
        const toHubY = hubY - my
        const dot = px * toHubX + py * toHubY
        const sign = dot >= 0 ? 1 : -1
        const bowAmount = Math.min(len * 0.25, 50)
        const cpx = mx + sign * px * bowAmount
        const cpy = my + sign * py * bowAmount

        ctx.strokeStyle = 'rgba(0,100,123,0.40)'
        ctx.lineWidth = 2
        ctx.setLineDash([6, 4])
        ctx.beginPath()
        ctx.moveTo(from.x, from.y)
        ctx.quadraticCurveTo(cpx, cpy, to.x, to.y)
        ctx.stroke()
        ctx.setLineDash([])
      }
    }
  }, [topology])

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
        <span style={{ color: color.onSurfaceMuted, fontWeight: 400 }}>// </span>mesh topology
        <span style={{
          fontSize: '0.75rem',
          fontWeight: 500,
          color: color.onSurfaceMuted,
          marginLeft: 12,
        }}>{topology?.nodes?.length ?? 0} nodes</span>
      </h2>

      <div style={{
        ...glass.card,
        borderRadius: radius.lg,
        overflow: 'hidden',
        position: 'relative',
        border: `1px solid ${color.outlineVariant}`,
        boxShadow: shadow.ambient,
      }}>
        <canvas ref={canvasRef} style={{ width: '100%', height: 420, display: 'block' }} />

        {/* Legend */}
        <div style={{
          position: 'absolute',
          bottom: 16,
          right: 20,
          display: 'flex',
          gap: 16,
          fontSize: '0.7rem',
          fontFamily: font.display,
          fontWeight: 500,
          color: color.onSurfaceMuted,
          ...glass.overlay,
          padding: '8px 16px',
          borderRadius: radius.sm,
          border: `1px solid ${color.outlineVariant}`,
        }}>
          <LegendDot color={color.active} label="Active" />
          <LegendDot color={color.stale} label="Stale" />
          <LegendDot color={color.primary} label="Registry" />
          <span>┄ mesh link</span>
        </div>
      </div>
    </div>
  )
}

function LegendDot({ color: c, label }: { color: string; label: string }) {
  return (
    <span style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
      <span style={{
        width: 8,
        height: 8,
        borderRadius: '50%',
        background: c,
        display: 'inline-block',
      }} />
      {label}
    </span>
  )
}
