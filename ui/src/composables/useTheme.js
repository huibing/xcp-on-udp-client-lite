/**
 * useTheme — singleton reactive theme store.
 * Persists to localStorage under 'xcp_global_theme'.
 * Import and call useTheme() from any component to share the same reactive state.
 */
import { ref, computed, watchEffect } from 'vue'

const STORAGE_KEY = 'xcp_global_theme'

// ── Reactive state (module-level singleton) ──────────────────────────────────
const theme    = ref('green')
const customFg = ref('#00cc44')
const customBg = ref('#090909')

// Load saved theme on module init
try {
  const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
  if (saved.theme)    theme.value    = saved.theme
  if (saved.customFg) customFg.value = saved.customFg
  if (saved.customBg) customBg.value = saved.customBg
} catch { /* ignore corrupt storage */ }

// Auto-persist whenever any value changes
watchEffect(() => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      theme:    theme.value,
      customFg: customFg.value,
      customBg: customBg.value,
    }))
  } catch { /* storage quota */ }
})

// ── Theme option list ────────────────────────────────────────────────────────
export const themeOptions = [
  { id: 'green',  label: 'Green / Black',    fg: '#00cc44', bg: '#090909' },
  { id: 'blue',   label: 'Gray / Dark Blue', fg: '#adb5bd', bg: '#1a2332' },
  { id: 'light',  label: 'VS Code Light',    fg: '#333333', bg: '#ffffff' },
  { id: 'custom', label: 'Custom',           fg: '#00cc44', bg: '#090909' },
]

// ── Color helpers ─────────────────────────────────────────────────────────────
export function hexToRgb(hex) {
  const h = hex.replace('#', '')
  return [parseInt(h.slice(0,2),16), parseInt(h.slice(2,4),16), parseInt(h.slice(4,6),16)]
}
export function rgbToHex(r,g,b) {
  return '#' + [r,g,b].map(v => Math.max(0,Math.min(255,v)).toString(16).padStart(2,'0')).join('')
}
export function mixHex(hex1, hex2, t) { // t=0→hex1, t=1→hex2
  const [r1,g1,b1] = hexToRgb(hex1), [r2,g2,b2] = hexToRgb(hex2)
  return rgbToHex(Math.round(r1+(r2-r1)*t), Math.round(g1+(g2-g1)*t), Math.round(b1+(b2-b1)*t))
}

// ── Custom theme CSS variable map ─────────────────────────────────────────────
export const customThemeStyle = computed(() => {
  if (theme.value !== 'custom') return {}
  const fg = customFg.value, bg = customBg.value
  const dark = (hexToRgb(bg).reduce((s,v,i) => s + v*[0.299,0.587,0.114][i], 0)) < 128
  const shade = dark ? '#000000' : '#aaaaaa'
  const anti  = dark ? '#ffffff' : '#000000'
  return {
    '--exp-bg':          bg,
    '--exp-bg2':         mixHex(bg, shade, 0.15),
    '--exp-bg3':         mixHex(bg, shade, 0.20),
    '--exp-bg4':         mixHex(bg, shade, 0.10),
    '--exp-bg5':         mixHex(bg, anti,  0.06),
    '--exp-fg':          fg,
    '--exp-fg-bright':   mixHex(fg, anti,  0.25),
    '--exp-fg-mid':      mixHex(fg, bg,    0.15),
    '--exp-fg-dim':      mixHex(fg, bg,    0.48),
    '--exp-fg-dim2':     mixHex(fg, bg,    0.55),
    '--exp-fg-dim3':     mixHex(fg, bg,    0.65),
    '--exp-fg-dim4':     mixHex(fg, bg,    0.75),
    '--exp-border':      mixHex(fg, bg,    0.80),
    '--exp-border2':     mixHex(fg, bg,    0.86),
    '--exp-border3':     mixHex(fg, bg,    0.91),
    '--exp-hover':       mixHex(fg, bg,    0.80),
    '--exp-ok-bg':       dark ? mixHex(fg, '#000000', 0.85) : mixHex(fg, '#ffffff', 0.80),
    '--exp-err-bg':      dark ? '#330011' : '#fde8e8',
    '--exp-working-bg':  mixHex(fg, bg,    0.75),
    '--exp-primary-bg':  mixHex(fg, bg,    0.80),
    '--exp-cal-hover':   mixHex(fg, bg,    0.86),
  }
})

// ── Composable export ─────────────────────────────────────────────────────────
export function useTheme() {
  return { theme, customFg, customBg, themeOptions, customThemeStyle, hexToRgb, mixHex, rgbToHex }
}
