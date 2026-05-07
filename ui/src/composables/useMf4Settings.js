import { ref, watch } from 'vue'

const STORAGE_KEY = 'xcp_global_mf4'

const mf4Dir = ref('records/')
const mf4Pattern = ref('{title}_{YYYYMMDD}_{HHMMSS}.mf4')

// Load persisted values once at module load time
try {
  const raw = localStorage.getItem(STORAGE_KEY)
  if (raw) {
    const s = JSON.parse(raw)
    if (s.mf4Dir)     mf4Dir.value     = s.mf4Dir
    if (s.mf4Pattern) mf4Pattern.value = s.mf4Pattern
  }
} catch {}

// Auto-persist whenever values change
watch([mf4Dir, mf4Pattern], () => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      mf4Dir:     mf4Dir.value,
      mf4Pattern: mf4Pattern.value,
    }))
  } catch {}
})

export function useMf4Settings() {
  return { mf4Dir, mf4Pattern }
}
