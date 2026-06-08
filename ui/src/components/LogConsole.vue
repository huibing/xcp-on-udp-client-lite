<template>
  <div class="console-panel" :style="{ height: panelHeight + 'px' }">
    <div class="console-resize-handle" @mousedown.prevent="startResize"></div>
    <div class="console-toolbar">
      <span class="console-levels">
        <label v-for="lvl in allLevels" :key="lvl" class="console-level-cb" :class="'cb-' + lvl.toLowerCase()">
          <input type="checkbox" :value="lvl" v-model="activeLevels" />
          {{ lvl }}
        </label>
      </span>
      <input class="console-search" v-model="searchText" placeholder="Search…" title="Search log messages (supports regex)" @keyup.enter="goNextMatch" @keyup.shift.enter="goPrevMatch" />
      <button class="console-btn console-regex-btn" :class="{ active: useRegex }" @click="useRegex = !useRegex" title="Toggle regex mode">.*</button>
      <span v-if="searchText && matchCount > 0" class="console-match-idx">{{ matchIdx + 1 }}/{{ matchCount }}</span>
      <span v-else-if="searchText && matchCount === 0" class="console-match-idx console-match-none">0/0</span>
      <button class="console-btn console-nav-btn" :disabled="!searchText || matchCount === 0" @click="goPrevMatch" title="Previous match (Shift+Enter)">▲</button>
      <button class="console-btn console-nav-btn" :disabled="!searchText || matchCount === 0" @click="goNextMatch" title="Next match (Enter)">▼</button>
      <span class="console-count">{{ filteredLogs.length }} entries</span>
      <div class="console-toolbar-spacer"></div>
      <button class="console-btn" @click="clearLogs" title="Clear all logs">🗑 Clear</button>
      <button class="console-btn" @click="scrollToBottom" v-if="!atBottom" title="Scroll to bottom">↓</button>
      <button class="console-btn console-close" @click="$emit('close')" title="Close console">✕</button>
    </div>
    <div class="console-body" ref="logContainer" @scroll="onScroll">
      <div v-for="entry in filteredLogs" :key="entry.id" class="log-row" :class="rowClass(entry)">
        <span class="log-time">{{ entry.timestamp }}</span>
        <span class="log-level" :class="'lvl-' + entry.level.toLowerCase()">{{ entry.level }}</span>
        <span class="log-target">{{ entry.target }}</span>
        <span class="log-msg">{{ entry.message }}</span>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const emit = defineEmits(['close'])

const allLevels = ['TRACE', 'DEBUG', 'INFO', 'WARN', 'ERROR']
const activeLevels = ref([...allLevels])

const panelHeight = ref(250)
const logEntries = ref([])
const lastId = ref(0)
const atBottom = ref(true)
const logContainer = ref(null)
const searchText = ref('')
const useRegex = ref(false)

let regexCache = null
let regexError = false
const currentMatchId = ref(null)

const filteredLogs = computed(() => {
  let result = logEntries.value
  const arr = activeLevels.value
  if (arr.length > 0 && arr.length < allLevels.length) {
    result = result.filter(e => arr.includes(e.level))
  }
  const text = searchText.value
  if (!text) return result
  if (useRegex.value) {
    try {
      if (!regexCache || regexCache.source !== text) {
        regexCache = new RegExp(text, 'i')
        regexError = false
      }
      return result.filter(e => regexCache.test(e.message) || regexCache.test(e.target))
    } catch {
      regexError = true
      return result
    }
  }
  const lower = text.toLowerCase()
  return result.filter(e => e.message.toLowerCase().includes(lower) || e.target.toLowerCase().includes(lower))
})

const matchCount = computed(() => {
  if (!searchText.value) return 0
  return filteredLogs.value.length
})

const matchIdx = computed(() => {
  if (currentMatchId.value === null) return 0
  const idx = filteredLogs.value.findIndex(e => e.id === currentMatchId.value)
  return idx >= 0 ? idx : 0
})

function goNextMatch() {
  if (matchCount.value === 0) return
  const list = filteredLogs.value
  if (currentMatchId.value === null) {
    currentMatchId.value = list[0].id
  } else {
    const idx = list.findIndex(e => e.id === currentMatchId.value)
    currentMatchId.value = list[(idx + 1) % list.length].id
  }
  scrollMatchIntoView()
}

function goPrevMatch() {
  if (matchCount.value === 0) return
  const list = filteredLogs.value
  if (currentMatchId.value === null) {
    currentMatchId.value = list[list.length - 1].id
  } else {
    const idx = list.findIndex(e => e.id === currentMatchId.value)
    currentMatchId.value = list[(idx - 1 + list.length) % list.length].id
  }
  scrollMatchIntoView()
}

function rowClass(entry) {
  const cls = 'log-' + entry.level.toLowerCase()
  if (entry.id === currentMatchId.value) return cls + ' log-highlight'
  return cls
}

function scrollMatchIntoView() {
  requestAnimationFrame(() => {
    const el = logContainer.value
    if (!el || currentMatchId.value === null) return
    const row = el.querySelector(`.log-row:nth-child(${matchIdx.value + 1})`)
    if (row) row.scrollIntoView({ block: 'nearest' })
  })
}

watch(searchText, () => { currentMatchId.value = null })

function onScroll() {
  const el = logContainer.value
  if (!el) return
  atBottom.value = el.scrollHeight - el.scrollTop - el.clientHeight < 30
}

function scrollToBottom() {
  const el = logContainer.value
  if (!el) return
  el.scrollTop = el.scrollHeight
  atBottom.value = true
}

async function clearLogs() {
  await invoke('console_logs_clear')
  logEntries.value = []
  lastId.value = 0
}

async function poll() {
  try {
    const entries = await invoke('console_logs_get', { sinceId: lastId.value })
    if (entries.length > 0) {
      lastId.value = entries[entries.length - 1].id
      logEntries.value.push(...entries)
      if (atBottom.value) {
        // Wait for DOM update, then scroll
        requestAnimationFrame(() => {
          const el = logContainer.value
          if (el) el.scrollTop = el.scrollHeight
        })
      }
    }
  } catch (e) {
    // Silently ignore polling errors (e.g. during app shutdown)
  }
}

let pollTimer = null

function startResize(e) {
  const startY = e.clientY
  const startH = panelHeight.value
  function onMove(ev) {
    const dy = startY - ev.clientY
    panelHeight.value = Math.max(100, Math.min(800, startH + dy))
  }
  function onUp() {
    document.removeEventListener('mousemove', onMove)
    document.removeEventListener('mouseup', onUp)
  }
  document.addEventListener('mousemove', onMove)
  document.addEventListener('mouseup', onUp)
}

onMounted(() => {
  poll()
  pollTimer = setInterval(poll, 200)
})

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer)
})
</script>

<style scoped>
.console-panel {
  background: var(--exp-bg2, #060606);
  border-top: 1px solid var(--exp-border, #003311);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  position: relative;
}

.console-resize-handle {
  height: 4px;
  cursor: ns-resize;
  background: var(--exp-border3, #001a09);
  flex-shrink: 0;
}
.console-resize-handle:hover {
  background: var(--exp-fg-mid, #00aa33);
}

.console-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 8px;
  background: var(--exp-bg3, #0d0d0d);
  border-bottom: 1px solid var(--exp-border2, #002211);
  flex-shrink: 0;
}

.console-levels {
  display: flex;
  gap: 2px;
  align-items: center;
}
.console-level-cb {
  display: flex;
  align-items: center;
  gap: 2px;
  cursor: pointer;
  font-size: 0.75rem;
  padding: 1px 5px;
  border: 1px solid transparent;
  border-radius: 2px;
  user-select: none;
}
.console-level-cb:hover {
  border-color: var(--exp-border, #003311);
}
.console-level-cb input {
  accent-color: var(--exp-fg-mid, #00aa33);
  margin: 0;
  cursor: pointer;
}
.cb-trace { color: #888888; }
.cb-debug { color: #66aacc; }
.cb-info  { color: var(--exp-fg-mid, #00aa33); }
.cb-warn  { color: #ccaa00; }
.cb-error { color: #cc3333; }

.console-search {
  background: var(--exp-bg, #090909);
  color: var(--exp-fg, #00cc44);
  border: 1px solid var(--exp-border, #003311);
  padding: 2px 6px;
  font-family: inherit;
  font-size: 0.8rem;
  width: 160px;
  outline: none;
}
.console-search:focus {
  border-color: var(--exp-fg-mid, #00aa33);
}
.console-search::placeholder {
  color: var(--exp-fg-dim4, #335522);
}

.console-regex-btn {
  font-weight: bold;
  letter-spacing: 1px;
  min-width: 34px;
  text-align: center;
}
.console-regex-btn.active {
  background: var(--exp-fg-mid, #00aa33);
  color: var(--exp-bg, #090909);
  border-color: var(--exp-fg-mid, #00aa33);
}

.console-match-idx {
  color: var(--exp-fg-bright, #00ff66);
  font-size: 0.78rem;
  min-width: 40px;
  text-align: center;
}
.console-match-none {
  color: var(--exp-fg-dim4, #446633);
}

.console-nav-btn {
  padding: 2px 6px;
  line-height: 1;
  font-size: 0.65rem;
}
.console-nav-btn:disabled {
  opacity: 0.3;
  cursor: default;
}
.console-nav-btn:disabled:hover {
  background: none;
  color: var(--exp-fg-mid, #00aa33);
}

.console-count {
  color: var(--exp-fg-dim, #556644);
  font-size: 0.75rem;
}

.console-toolbar-spacer {
  flex: 1;
}

.console-btn {
  background: none;
  border: 1px solid var(--exp-border, #003311);
  color: var(--exp-fg-mid, #00aa33);
  padding: 2px 8px;
  cursor: pointer;
  font-family: inherit;
  font-size: 0.8rem;
  border-radius: 2px;
}
.console-btn:hover {
  background: var(--exp-hover, #003311);
  color: var(--exp-fg-bright, #00ff66);
}
.console-close {
  border: none;
  font-size: 0.9rem;
  padding: 2px 6px;
}

.console-body {
  flex: 1;
  overflow-y: auto;
  padding: 2px 0;
  font-size: 0.78rem;
  line-height: 1.5;
}

.log-row {
  display: flex;
  gap: 8px;
  padding: 1px 8px;
  white-space: nowrap;
}
.log-row:hover {
  background: var(--exp-hover, #003311);
}
.log-highlight {
  background: var(--exp-primary-bg, #004433) !important;
  outline: 1px solid var(--exp-fg-mid, #00aa33);
  outline-offset: -1px;
}

.log-time {
  color: var(--exp-fg-dim2, #558844);
  flex-shrink: 0;
  min-width: 90px;
}

.log-level {
  flex-shrink: 0;
  min-width: 44px;
  font-weight: bold;
}
.lvl_trace  { color: #888888; }
.lvl_debug  { color: #66aacc; }
.lvl_info   { color: var(--exp-fg-mid, #00aa33); }
.lvl_warn   { color: #ccaa00; }
.lvl_error  { color: #cc3333; }

.log-target {
  color: var(--exp-fg-dim3, #446633);
  flex-shrink: 0;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.log-msg {
  color: var(--exp-fg, #00cc44);
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Level row tints */
.log-trace { opacity: 0.7; }
.log-debug { opacity: 0.85; }
</style>
