<template>
  <div class="console-panel" :style="{ height: panelHeight + 'px' }">
    <div class="console-resize-handle" @mousedown.prevent="startResize"></div>
    <div class="console-toolbar">
      <select v-model="levelFilter" class="console-filter">
        <option value="">ALL</option>
        <option value="TRACE">TRACE</option>
        <option value="DEBUG">DEBUG</option>
        <option value="INFO">INFO</option>
        <option value="WARN">WARN</option>
        <option value="ERROR">ERROR</option>
      </select>
      <span class="console-count">{{ filteredLogs.length }} entries</span>
      <div class="console-toolbar-spacer"></div>
      <button class="console-btn" @click="clearLogs" title="Clear all logs">🗑 Clear</button>
      <button class="console-btn" @click="scrollToBottom" v-if="!atBottom" title="Scroll to bottom">↓</button>
      <button class="console-btn console-close" @click="$emit('close')" title="Close console">✕</button>
    </div>
    <div class="console-body" ref="logContainer" @scroll="onScroll">
      <div v-for="entry in filteredLogs" :key="entry.id" class="log-row" :class="'log-' + entry.level.toLowerCase()">
        <span class="log-time">{{ entry.timestamp }}</span>
        <span class="log-level" :class="'lvl-' + entry.level.toLowerCase()">{{ entry.level }}</span>
        <span class="log-target">{{ entry.target }}</span>
        <span class="log-msg">{{ entry.message }}</span>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const emit = defineEmits(['close'])

const panelHeight = ref(250)
const levelFilter = ref('')
const logEntries = ref([])
const lastId = ref(0)
const atBottom = ref(true)
const logContainer = ref(null)

const filteredLogs = computed(() => {
  if (!levelFilter.value) return logEntries.value
  return logEntries.value.filter(e => e.level === levelFilter.value)
})

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

.console-filter {
  background: var(--exp-bg, #090909);
  color: var(--exp-fg, #00cc44);
  border: 1px solid var(--exp-border, #003311);
  padding: 2px 6px;
  font-family: inherit;
  font-size: 0.8rem;
  cursor: pointer;
}
.console-filter:focus {
  outline: none;
  border-color: var(--exp-fg-mid, #00aa33);
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
