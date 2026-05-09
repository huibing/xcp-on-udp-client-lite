<template>
  <div class="page">
    <div class="page-header">
      <div>
        <h1>MF4 Viewer</h1>
        <p class="page-sub">Offline MF4 signal viewer. This page only reads recorded files and does not depend on any live device connection.</p>
      </div>
      <div class="page-actions">
        <button class="secondary" @click="refreshRecordFiles" :disabled="loadingList">
          {{ loadingList ? 'Refreshing...' : '↻ Refresh List' }}
        </button>
        <button @click="browseMf4File" :disabled="openingFile !== '' || loadingSignal !== ''">Open MF4…</button>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>

    <div class="layout">
      <section class="panel file-panel">
        <div class="panel-header">
          <div>
            <h2>Recorded Files</h2>
            <div class="panel-meta">{{ recordDirectory || 'records/' }}</div>
          </div>
          <span class="badge">{{ recordFiles.length }}</span>
        </div>

        <div v-if="!directoryExists" class="empty-state">
          Directory not found yet. Set a recording folder in Settings or open an MF4 file manually.
        </div>
        <div v-else-if="loadingList" class="empty-state">Loading MF4 files…</div>
        <div v-else-if="recordFiles.length === 0" class="empty-state">No MF4 files found in the configured recording directory.</div>

        <div v-else class="file-list">
          <button
            v-for="file in recordFiles"
            :key="file.path"
            class="file-row"
            :class="{ active: activeFile?.path === file.path }"
            @click="openMf4File(file.path)"
          >
            <span class="file-main">
              <span class="file-name">{{ file.name }}</span>
              <span class="file-size">{{ formatBytes(file.size_bytes) }}</span>
            </span>
            <span class="file-path">{{ file.path }}</span>
          </button>
        </div>
      </section>

      <section class="panel channel-panel">
        <div class="panel-header">
          <div>
            <h2>Channels</h2>
            <div class="panel-meta">
              {{ activeFile ? `${activeFile.name} · ${activeChannels.length} channel(s)` : 'Open an MF4 file to browse channels' }}
            </div>
          </div>
          <span v-if="openingFile" class="badge loading">Loading…</span>
        </div>

        <input
          v-model.trim="channelSearch"
          class="inp channel-search"
          :disabled="!activeFile"
          placeholder="Search channel name…"
        />

        <div v-if="!activeFile" class="empty-state">Select a file from the left list or open an MF4 file manually.</div>
        <div v-else-if="filteredChannels.length === 0" class="empty-state">No matching channels.</div>

        <div v-else class="channel-list">
          <div
            v-for="channel in filteredChannels"
            :key="`${activeFile.path}::${channel}`"
            class="channel-row"
            @dblclick="addSignal(channel)"
          >
            <span class="channel-name">{{ channel }}</span>
            <button
              class="sm"
              :disabled="loadingSignal === signalKey(activeFile.path, channel) || loadedSignalKeys.has(signalKey(activeFile.path, channel))"
              @click="addSignal(channel)"
            >
              {{
                loadedSignalKeys.has(signalKey(activeFile.path, channel))
                  ? 'Loaded'
                  : loadingSignal === signalKey(activeFile.path, channel)
                    ? 'Loading…'
                    : 'Add'
              }}
            </button>
          </div>
        </div>
      </section>

      <section class="panel chart-panel">
        <div class="panel-header">
          <div>
            <h2>Signal Graph</h2>
            <div class="panel-meta">{{ loadedSignals.length }} loaded signal(s)</div>
          </div>
          <button v-if="loadedSignals.length > 0" class="secondary sm" @click="clearSignals">Clear All</button>
        </div>

        <div v-if="loadedSignals.length === 0" class="empty-state chart-empty">
          Add one or more channels to render their recorded waveform here.
        </div>
        <template v-else>
          <div class="loaded-tags">
            <span v-for="signal in loadedSignals" :key="signal.key" class="signal-tag">
              <span class="signal-color" :style="{ background: signal.color }"></span>
              <span class="signal-label">{{ signal.name }}</span>
              <span class="signal-file">{{ signal.device_name }}</span>
              <button class="tag-close" @click="removeSignal(signal.key)">✕</button>
            </span>
          </div>

          <SignalChart
            :signals="loadedSignals"
            :historyStore="historyStore"
            :running="false"
            :startEpoch="null"
            :gridColor="chartGridColor"
            :tickColor="chartTickColor"
            @close="clearSignals"
          />

          <div class="stats-card">
            <table class="stats-table">
              <thead>
                <tr>
                  <th>Signal</th>
                  <th>File</th>
                  <th>Samples</th>
                  <th>Min</th>
                  <th>Max</th>
                  <th>Latest</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in loadedSignalRows" :key="row.key">
                  <td>{{ row.name }}</td>
                  <td>{{ row.file }}</td>
                  <td>{{ row.samples }}</td>
                  <td>{{ row.min }}</td>
                  <td>{{ row.max }}</td>
                  <td>{{ row.latest }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </template>
      </section>
    </div>
  </div>
</template>

<script setup>
import { computed, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

import SignalChart from './SignalChart.vue'
import { useMf4Settings } from '../composables/useMf4Settings.js'
import { useTheme } from '../composables/useTheme.js'

const SIGNAL_COLORS = ['#00ff66', '#4dd2ff', '#ffcc00', '#ff6688', '#b388ff', '#ff9966', '#66e0a3']

const { mf4Dir } = useMf4Settings()
const { theme } = useTheme()

const recordFiles = ref([])
const recordDirectory = ref('')
const directoryExists = ref(true)
const activeFile = ref(null)
const activeChannels = ref([])
const channelSearch = ref('')
const loadingList = ref(false)
const openingFile = ref('')
const loadingSignal = ref('')
const msg = ref('')
const msgErr = ref(false)
const loadedSignals = ref([])
const historyStore = new Map()

const loadedSignalKeys = computed(() => new Set(loadedSignals.value.map(signal => signal.key)))

const filteredChannels = computed(() => {
  const search = channelSearch.value.trim().toLowerCase()
  if (!search) return activeChannels.value
  return activeChannels.value.filter(channel => channel.toLowerCase().includes(search))
})

const loadedSignalRows = computed(() => loadedSignals.value.map(signal => {
  const history = historyStore.get(signal.key)
  const values = history?.values ?? []
  const count = values.length ?? 0
  if (!count) {
    return {
      key: signal.key,
      name: signal.name,
      file: signal.device_name,
      samples: '0',
      min: '—',
      max: '—',
      latest: '—',
    }
  }

  let min = Number.POSITIVE_INFINITY
  let max = Number.NEGATIVE_INFINITY
  for (let i = 0; i < values.length; i++) {
    const value = Number(values[i])
    if (value < min) min = value
    if (value > max) max = value
  }

  return {
    key: signal.key,
    name: signal.name,
    file: signal.device_name,
    samples: count.toLocaleString(),
    min: formatValue(min),
    max: formatValue(max),
    latest: formatValue(values[values.length - 1]),
  }
}))

const chartGridColor = computed(() => {
  if (theme.value === 'light') return 'rgba(0,0,0,0.1)'
  if (theme.value === 'blue') return 'rgba(100,140,200,0.15)'
  return 'rgba(0,200,100,0.1)'
})

const chartTickColor = computed(() => {
  if (theme.value === 'light') return '#555555'
  if (theme.value === 'blue') return '#8fadc8'
  return '#00aa33'
})

function setMsg(text, err = false) {
  msg.value = text
  msgErr.value = err
  window.clearTimeout(setMsg.timer)
  setMsg.timer = window.setTimeout(() => {
    msg.value = ''
  }, 5000)
}

setMsg.timer = 0

function fileNameFromPath(path) {
  return String(path).split(/[/\\]/).pop() || String(path)
}

function signalKey(filePath, channelName) {
  return `${filePath}::${channelName}`
}

function formatBytes(value) {
  const bytes = Number(value || 0)
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`
}

function formatValue(value) {
  const num = Number(value)
  if (!Number.isFinite(num)) return '—'
  if (Math.abs(num) >= 10000 || (Math.abs(num) > 0 && Math.abs(num) < 0.001)) {
    return num.toExponential(3)
  }
  return num.toFixed(4)
}

function sortFiles(files) {
  return [...files].sort((left, right) => left.name.localeCompare(right.name, undefined, { sensitivity: 'base' }))
}

function upsertRecordFile(file) {
  if (!file?.path) return
  const next = recordFiles.value.filter(item => item.path !== file.path)
  next.push({
    name: file.name || fileNameFromPath(file.path),
    path: file.path,
    size_bytes: file.size_bytes ?? 0,
  })
  recordFiles.value = sortFiles(next)
}

function pickColor(index) {
  return SIGNAL_COLORS[index % SIGNAL_COLORS.length]
}

function toFloat64Array(payload) {
  let uint8
  if (payload instanceof Uint8Array) {
    uint8 = payload
  } else if (payload instanceof ArrayBuffer) {
    uint8 = new Uint8Array(payload)
  } else if (ArrayBuffer.isView(payload)) {
    uint8 = new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength)
  } else if (Array.isArray(payload)) {
    uint8 = Uint8Array.from(payload)
  } else if (payload && Array.isArray(payload.data)) {
    uint8 = Uint8Array.from(payload.data)
  } else {
    throw new Error('Unexpected MF4 binary payload format')
  }

  if (uint8.byteLength % 8 !== 0) {
    throw new Error('MF4 payload length is not aligned to f64 samples')
  }

  const buffer = uint8.buffer.slice(uint8.byteOffset, uint8.byteOffset + uint8.byteLength)
  return new Float64Array(buffer)
}

function singlePath(result) {
  if (!result) return null
  return Array.isArray(result) ? (result[0] ?? null) : result
}

async function refreshRecordFiles() {
  loadingList.value = true
  try {
    const response = await invoke('mf4_list_record_files', { directory: mf4Dir.value || 'records' })
    recordDirectory.value = response.directory || ''
    directoryExists.value = response.exists !== false
    recordFiles.value = sortFiles(response.files || [])
  } catch (error) {
    setMsg(String(error), true)
  } finally {
    loadingList.value = false
  }
}

async function openMf4File(filePath) {
  openingFile.value = filePath
  try {
    const response = await invoke('mf4_open_file', { mf4Path: filePath })
    activeFile.value = response.file
    activeChannels.value = response.channels || []
    channelSearch.value = ''
    upsertRecordFile(response.file)
    setMsg(`Loaded ${response.channel_count ?? activeChannels.value.length} channel(s) from ${response.file?.name || fileNameFromPath(filePath)}`)
  } catch (error) {
    setMsg(String(error), true)
  } finally {
    openingFile.value = ''
  }
}

async function browseMf4File() {
  try {
    const picked = singlePath(await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'MF4 files', extensions: ['mf4', 'MF4'] }],
    }))
    if (!picked) return
    await openMf4File(String(picked))
  } catch (error) {
    setMsg(String(error), true)
  }
}

async function addSignal(channelName) {
  if (!activeFile.value) return
  const key = signalKey(activeFile.value.path, channelName)
  if (loadedSignalKeys.value.has(key)) return

  loadingSignal.value = key
  try {
    const [rawValues, rawTimes] = await Promise.all([
      invoke('mf4_get_channel_data', { mf4Path: activeFile.value.path, channelName }),
      invoke('mf4_get_channel_time', { mf4Path: activeFile.value.path, channelName }),
    ])

    const values = toFloat64Array(rawValues)
    const times = toFloat64Array(rawTimes)
    if (values.length === 0 || times.length === 0) {
      throw new Error(`Channel '${channelName}' did not return any samples`)
    }
    if (values.length !== times.length) {
      throw new Error(`Channel '${channelName}' has mismatched data/time lengths`)
    }

    historyStore.set(key, { times, values })
    loadedSignals.value = [
      ...loadedSignals.value,
      {
        key,
        name: channelName,
        device_name: activeFile.value.name,
        color: pickColor(loadedSignals.value.length),
        path: activeFile.value.path,
      },
    ]
  } catch (error) {
    setMsg(String(error), true)
  } finally {
    loadingSignal.value = ''
  }
}

function removeSignal(key) {
  historyStore.delete(key)
  loadedSignals.value = loadedSignals.value.filter(signal => signal.key !== key)
}

function clearSignals() {
  historyStore.clear()
  loadedSignals.value = []
}

watch(() => mf4Dir.value, () => {
  refreshRecordFiles()
})

onMounted(() => {
  refreshRecordFiles()
})
</script>

<style scoped>
.page {
  color: var(--exp-fg, #00cc44);
}

.page-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 16px;
  flex-wrap: wrap;
}

.page-header h1 {
  color: var(--exp-fg-bright, #00ff66);
  font-size: 1.45rem;
  margin: 0 0 6px;
}

.page-sub {
  color: var(--exp-fg-dim, #556644);
  font-size: 0.9rem;
  max-width: 760px;
}

.page-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.layout {
  display: grid;
  grid-template-columns: minmax(240px, 280px) minmax(260px, 340px) minmax(420px, 1fr);
  gap: 14px;
  align-items: start;
}

.panel,
.stats-card {
  background: var(--exp-bg3, #0d0d0d);
  border: 1px solid var(--exp-border, #003311);
  border-radius: 6px;
}

.panel {
  padding: 14px;
  min-height: 620px;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 10px;
  margin-bottom: 12px;
}

.panel-header h2 {
  margin: 0;
  color: var(--exp-fg-bright, #00ff66);
  font-size: 1rem;
}

.panel-meta {
  color: var(--exp-fg-dim3, #335522);
  font-size: 0.78rem;
  margin-top: 3px;
  word-break: break-all;
}

.badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  padding: 3px 8px;
  border: 1px solid var(--exp-border2, #002211);
  border-radius: 999px;
  color: var(--exp-fg, #00cc44);
  font-size: 0.78rem;
}

.badge.loading {
  color: #ffcc00;
  border-color: #664c00;
}

.file-list,
.channel-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 560px;
  overflow: auto;
}

.file-row {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
  width: 100%;
  text-align: left;
  padding: 10px;
  border-radius: 6px;
  border: 1px solid var(--exp-border2, #002211);
  background: var(--exp-bg2, #060606);
}

.file-row.active {
  border-color: var(--exp-fg-bright, #00ff66);
  background: var(--exp-hover, #003311);
}

.file-main {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
}

.file-name {
  color: var(--exp-fg-bright, #00ff66);
  font-weight: 700;
}

.file-size,
.file-path {
  color: var(--exp-fg-dim, #556644);
  font-size: 0.76rem;
}

.file-path {
  word-break: break-all;
}

.channel-search,
.inp {
  width: 100%;
  margin-bottom: 12px;
  background: var(--exp-bg2, #060606);
  color: var(--exp-fg, #00cc44);
  border: 1px solid var(--exp-border, #003311);
  border-radius: 4px;
  padding: 7px 10px;
  font-family: 'Consolas', 'Courier New', monospace;
  outline: none;
}

.channel-search:focus,
.inp:focus {
  border-color: var(--exp-fg-bright, #00ff66);
}

.channel-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--exp-border2, #002211);
  border-radius: 5px;
  background: var(--exp-bg2, #060606);
}

.channel-name {
  color: var(--exp-fg, #00cc44);
  word-break: break-word;
}

.chart-panel {
  min-height: 620px;
}

.chart-empty {
  min-height: 180px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.loaded-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 10px;
}

.signal-tag {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  border-radius: 999px;
  border: 1px solid var(--exp-border2, #002211);
  background: var(--exp-bg2, #060606);
  color: var(--exp-fg, #00cc44);
  font-size: 0.8rem;
}

.signal-color {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}

.signal-label {
  color: var(--exp-fg-bright, #00ff66);
}

.signal-file {
  color: var(--exp-fg-dim, #556644);
}

.tag-close {
  border: none;
  background: transparent;
  color: var(--exp-fg-dim3, #335522);
  cursor: pointer;
  padding: 0 2px;
}

.tag-close:hover {
  color: #ff6666;
}

.stats-card {
  margin-top: 12px;
  overflow: hidden;
}

.stats-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.82rem;
}

.stats-table th,
.stats-table td {
  padding: 8px 10px;
  border-bottom: 1px solid var(--exp-border2, #002211);
  text-align: left;
}

.stats-table th {
  color: var(--exp-fg-dim3, #335522);
  background: var(--exp-bg2, #060606);
}

.stats-table td {
  color: var(--exp-fg, #00cc44);
}

.empty-state {
  color: var(--exp-fg-dim, #556644);
  padding: 24px 8px;
  text-align: center;
}

.msg {
  margin-bottom: 12px;
  color: var(--exp-fg, #00cc44);
  font-size: 0.85rem;
}

.msg.err {
  color: #ff5555;
}

button {
  background: var(--exp-border3, #001a09);
  border: 1px solid var(--exp-fg, #00cc44);
  color: var(--exp-fg, #00cc44);
  padding: 6px 10px;
  cursor: pointer;
  font-family: 'Consolas', 'Courier New', monospace;
  font-size: 0.85rem;
}

button:hover:not(:disabled) {
  background: var(--exp-hover, #003311);
}

button:disabled {
  opacity: 0.45;
  cursor: default;
}

button.secondary,
button.sm {
  font-size: 0.78rem;
}

@media (max-width: 1400px) {
  .layout {
    grid-template-columns: 1fr;
  }

  .panel {
    min-height: auto;
  }
}
</style>
