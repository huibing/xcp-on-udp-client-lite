<template>
  <div class="packet-panel" :style="{ height: panelHeight + 'px' }">
    <div class="packet-resize-handle" @mousedown.prevent="startResize"></div>
    <div class="packet-toolbar">
      <label class="packet-toggle" title="Enable packet capture">
        <input type="checkbox" v-model="enabled" @change="toggleEnabled" />
        Capture
      </label>
      <select v-model="sourceFilter" class="packet-filter" title="Filter by source">
        <option value="">All sources</option>
        <option v-for="s in sources" :key="s" :value="s">{{ s }}</option>
      </select>
      <select v-model="dirFilter" class="packet-filter" title="Filter by direction">
        <option value="">All directions</option>
        <option value="outgoing">→ Outgoing</option>
        <option value="incoming">← Incoming</option>
      </select>
      <select v-model="classFilter" class="packet-filter" title="Filter by packet class">
        <option value="">All types</option>
        <option value="cto">CTO</option>
        <option value="daq">DAQ</option>
      </select>
      <input class="packet-search" v-model="searchText" placeholder="Search hex/desc…" />
      <span class="packet-count">{{ filteredEntries.length }} entries</span>
      <span v-if="dropped > 0" class="packet-dropped" title="Dropped entries (overflow)">+{{ dropped }} dropped</span>
      <div class="packet-spacer"></div>
      <button class="packet-btn" @click="clearPackets" title="Clear all packets">🗑 Clear</button>
      <button class="packet-btn" @click="scrollToBottom" v-if="!atBottom" title="Scroll to bottom">↓</button>
      <button class="packet-btn packet-close" @click="$emit('close')" title="Close packet browser">✕</button>
    </div>
    <div class="packet-body" ref="packetContainer" @scroll="onScroll">
      <div v-for="entry in filteredEntries" :key="entry.id" class="packet-row" :class="rowClass(entry)">
        <span class="packet-time">{{ entry.timestamp }}</span>
        <span class="packet-source" :title="entry.source">{{ entry.source }}</span>
        <span class="packet-dir" :class="'dir-' + entry.direction">
          {{ entry.direction === 'outgoing' ? '→' : '←' }}
        </span>
        <span class="packet-class" :class="'cls-' + entry.packet_class">
          {{ entry.packet_class === 'cto' ? 'CTO' : 'DAQ' }}
        </span>
        <span v-if="entry.daq_timestamp != null" class="packet-daq-ts" title="Slave timestamp">
          {{ entry.daq_timestamp }}
        </span>
        <span class="packet-hex">{{ entry.hex }}</span>
        <span class="packet-desc">{{ entry.description }}</span>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const emit = defineEmits(['close'])

const panelHeight = ref(250)
const packetEntries = ref([])
const lastId = ref(0)
const atBottom = ref(true)
const packetContainer = ref(null)
const enabled = ref(false)
const dropped = ref(0)

const sourceFilter = ref('')
const dirFilter = ref('')
const classFilter = ref('')
const searchText = ref('')

const sources = computed(() => {
  const s = new Set(packetEntries.value.map(e => e.source))
  return [...s].sort()
})

const filteredEntries = computed(() => {
  let result = packetEntries.value
  if (sourceFilter.value) {
    result = result.filter(e => e.source === sourceFilter.value)
  }
  if (dirFilter.value) {
    result = result.filter(e => e.direction === dirFilter.value)
  }
  if (classFilter.value) {
    result = result.filter(e => e.packet_class === classFilter.value)
  }
  if (searchText.value) {
    const lower = searchText.value.toLowerCase()
    result = result.filter(e =>
      e.hex.toLowerCase().includes(lower) ||
      e.description.toLowerCase().includes(lower)
    )
  }
  return result
})

function rowClass(entry) {
  return 'packet-' + entry.direction
}

function onScroll() {
  const el = packetContainer.value
  if (!el) return
  atBottom.value = el.scrollHeight - el.scrollTop - el.clientHeight < 30
}

function scrollToBottom() {
  const el = packetContainer.value
  if (!el) return
  el.scrollTop = el.scrollHeight
  atBottom.value = true
}

async function toggleEnabled() {
  await invoke('xcp_packet_set_enabled', { enabled: enabled.value })
}

async function clearPackets() {
  await invoke('xcp_packet_clear')
  packetEntries.value = []
  lastId.value = 0
  dropped.value = 0
}

async function poll() {
  try {
    const entries = await invoke('xcp_packet_get', { sinceId: lastId.value })
    if (entries.length > 0) {
      lastId.value = entries[entries.length - 1].id
      packetEntries.value.push(...entries)
      if (atBottom.value) {
        requestAnimationFrame(() => {
          const el = packetContainer.value
          if (el) el.scrollTop = el.scrollHeight
        })
      }
    }
  } catch (e) {
    // Silently ignore polling errors
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
.packet-panel {
  background: var(--exp-bg2, #060606);
  border-top: 1px solid var(--exp-border, #003311);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  position: relative;
}

.packet-resize-handle {
  height: 4px;
  cursor: ns-resize;
  background: var(--exp-border3, #001a09);
  flex-shrink: 0;
}
.packet-resize-handle:hover {
  background: var(--exp-fg-mid, #00aa33);
}

.packet-toolbar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  background: var(--exp-bg3, #0d0d0d);
  border-bottom: 1px solid var(--exp-border2, #002211);
  flex-shrink: 0;
  flex-wrap: wrap;
  font-size: 0.78rem;
}

.packet-toggle {
  display: flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
  user-select: none;
  color: var(--exp-fg-mid, #00aa33);
}
.packet-toggle input {
  accent-color: var(--exp-fg-mid, #00aa33);
  cursor: pointer;
  margin: 0;
}

.packet-filter, .packet-search {
  background: var(--exp-bg, #090909);
  color: var(--exp-fg, #00cc44);
  border: 1px solid var(--exp-border, #003311);
  padding: 2px 4px;
  font-family: inherit;
  font-size: 0.78rem;
  outline: none;
}
.packet-filter:focus, .packet-search:focus {
  border-color: var(--exp-fg-mid, #00aa33);
}
.packet-search {
  width: 130px;
}
.packet-search::placeholder {
  color: var(--exp-fg-dim4, #335522);
}

.packet-count {
  color: var(--exp-fg-dim, #556644);
  font-size: 0.75rem;
}

.packet-dropped {
  color: #ccaa00;
  font-size: 0.72rem;
}

.packet-spacer {
  flex: 1;
}

.packet-btn {
  background: none;
  border: 1px solid var(--exp-border, #003311);
  color: var(--exp-fg-mid, #00aa33);
  padding: 2px 8px;
  cursor: pointer;
  font-family: inherit;
  font-size: 0.78rem;
  border-radius: 2px;
}
.packet-btn:hover {
  background: var(--exp-hover, #003311);
  color: var(--exp-fg-bright, #00ff66);
}
.packet-close {
  border: none;
  font-size: 0.9rem;
  padding: 2px 6px;
}

.packet-body {
  flex: 1;
  overflow-y: auto;
  padding: 2px 0;
  font-size: 0.78rem;
  line-height: 1.5;
  font-family: 'Consolas', 'Courier New', monospace;
}

.packet-row {
  display: flex;
  gap: 8px;
  padding: 1px 8px;
  white-space: nowrap;
  align-items: baseline;
}
.packet-row:hover {
  background: var(--exp-hover, #003311);
}

.packet-outgoing {
  color: var(--exp-fg, #00cc44);
}
.packet-incoming {
  color: #66ccff;
}

.packet-time {
  color: var(--exp-fg-dim2, #558844);
  flex-shrink: 0;
  min-width: 80px;
}

.packet-source {
  color: var(--exp-fg-dim3, #446633);
  flex-shrink: 0;
  min-width: 80px;
  max-width: 100px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.packet-dir {
  flex-shrink: 0;
  min-width: 16px;
  text-align: center;
  font-weight: bold;
}
.dir-outgoing { color: var(--exp-fg, #00cc44); }
.dir-incoming { color: #66ccff; }

.packet-class {
  flex-shrink: 0;
  min-width: 32px;
  font-size: 0.7rem;
  font-weight: bold;
}
.cls-cto { color: #ccaa00; }
.cls-daq { color: #cc66cc; }

.packet-daq-ts {
  color: #cc66cc;
  flex-shrink: 0;
  min-width: 40px;
  font-size: 0.7rem;
}

.packet-hex {
  flex-shrink: 0;
  color: inherit;
}

.packet-desc {
  color: inherit;
  opacity: 0.85;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
