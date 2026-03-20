/** Atmospheric Luminescence — Design Tokens */

export const color = {
  surface: '#f5f7f9',
  surfaceContainerLow: '#eef1f3',
  surfaceContainer: '#e5e9eb',
  surfaceContainerLowest: '#ffffff',

  primary: '#00647b',
  primaryContainer: '#00cffc',
  onPrimary: '#ffffff',
  onSurface: '#2c2f31',
  onSurfaceMuted: '#5f6368',

  outlineVariant: 'rgba(171,173,175,0.15)',  // ghost border

  error: '#fb5151',
  errorContainer: 'rgba(251,81,81,0.10)',

  // Semantic / health
  active: '#16a34a',
  stale: '#d97706',
  offline: '#dc2626',
  info: '#0284c7',
} as const

export const gradient = {
  primary: 'linear-gradient(135deg, #00647b, #00cffc)',
  primarySubtle: 'linear-gradient(135deg, rgba(0,100,123,0.06), rgba(0,207,252,0.08))',
} as const

export const font = {
  display: "'Space Grotesk', sans-serif",
  body: "'Manrope', sans-serif",
  mono: "'JetBrains Mono', monospace",
} as const

export const radius = {
  sm: '0.75rem',
  md: '1.5rem',
  lg: '2rem',
  xl: '3rem',
} as const

export const spacing = {
  1: '0.25rem',
  2: '0.5rem',
  3: '0.75rem',
  4: '1.4rem',
  5: '1.6rem',
  6: '2rem',
  8: '2.5rem',
  10: '3rem',
  12: '4rem',
} as const

export const shadow = {
  ambient: '0 8px 40px rgba(44,47,49,0.06)',
  float: '0 12px 64px rgba(44,47,49,0.08)',
  primaryGlow: '0 8px 32px rgba(0,207,252,0.18)',
} as const

export const glass = {
  header: {
    background: 'rgba(255,255,255,0.60)',
    backdropFilter: 'blur(32px)',
    WebkitBackdropFilter: 'blur(32px)',
  },
  card: {
    background: 'rgba(255,255,255,0.80)',
    backdropFilter: 'blur(24px)',
    WebkitBackdropFilter: 'blur(24px)',
  },
  overlay: {
    background: 'rgba(255,255,255,0.50)',
    backdropFilter: 'blur(24px)',
    WebkitBackdropFilter: 'blur(24px)',
  },
} as const
