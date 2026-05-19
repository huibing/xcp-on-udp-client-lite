import { ref, watch } from 'vue'

const STORAGE_KEY = 'xcp_global_csv_export'

/** 'comma' → ','   |   'semicolon' → ';'   |   'colon' → ':' */
const csvSeparator = ref('comma')

/** 'text' → enum labels   |   'raw' → numeric values */
const enumExportMode = ref('text')

try {
  const raw = localStorage.getItem(STORAGE_KEY)
  if (raw) {
    const s = JSON.parse(raw)
    if (s.csvSeparator)   csvSeparator.value   = s.csvSeparator
    if (s.enumExportMode) enumExportMode.value = s.enumExportMode
  }
} catch {}

watch([csvSeparator, enumExportMode], () => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      csvSeparator:   csvSeparator.value,
      enumExportMode: enumExportMode.value,
    }))
  } catch {}
})

export function useCsvExportSettings() {
  return { csvSeparator, enumExportMode }
}
