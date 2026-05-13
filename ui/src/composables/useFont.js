/**
 * useFont — singleton font store.
 * Persists to localStorage under 'xcp_global_font'.
 * Applies --exp-font and --exp-font-size CSS variables to :root on every change.
 */
import { ref, computed, watchEffect } from 'vue'

const STORAGE_KEY = 'xcp_global_font'

export const fontOptions = [
  { id: 'consolas',   label: 'Consolas',          value: 'Consolas, "Courier New", monospace' },
  { id: 'cascadia',   label: 'Cascadia Code',      value: '"Cascadia Code", Consolas, monospace' },
  { id: 'jetbrains',  label: 'JetBrains Mono',     value: '"JetBrains Mono", Consolas, monospace' },
  { id: 'fira',       label: 'Fira Code',          value: '"Fira Code", Consolas, monospace' },
  { id: 'sourcecodepro', label: 'Source Code Pro', value: '"Source Code Pro", Consolas, monospace' },
  { id: 'courier',    label: 'Courier New',        value: '"Courier New", Courier, monospace' },
  { id: 'custom',     label: 'Custom…',            value: '' },
]

export const fontSizeOptions = [
  { label: '11px', value: '11' },
  { label: '12px', value: '12' },
  { label: '13px', value: '13' },
  { label: '14px', value: '14' },
  { label: '15px', value: '15' },
]

const fontId     = ref('consolas')
const customFont = ref('')
const fontSize   = ref('13')

// Load persisted values
try {
  const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
  if (saved.fontId)     fontId.value     = saved.fontId
  if (saved.customFont) customFont.value = saved.customFont
  if (saved.fontSize)   fontSize.value   = saved.fontSize
} catch { /* ignore corrupt storage */ }

export const currentFontFamily = computed(() => {
  if (fontId.value === 'custom') return customFont.value || 'monospace'
  return fontOptions.find(f => f.id === fontId.value)?.value ?? 'Consolas, monospace'
})

// Apply to :root as CSS variables + persist whenever values change
watchEffect(() => {
  document.documentElement.style.setProperty('--exp-font',      currentFontFamily.value)
  document.documentElement.style.setProperty('--exp-font-size', fontSize.value + 'px')
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      fontId:     fontId.value,
      customFont: customFont.value,
      fontSize:   fontSize.value,
    }))
  } catch { /* storage quota */ }
})

export function useFont() {
  return { fontId, customFont, fontSize, fontOptions, fontSizeOptions, currentFontFamily }
}
