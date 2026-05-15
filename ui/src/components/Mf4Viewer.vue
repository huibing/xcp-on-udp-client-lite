<template>
  <div class="page">
    <div class="page-header">
      <div>
        <h1>MF4 Viewer</h1>
        <p class="page-sub">Offline MF4 signal viewer. This page only reads recorded files and does not depend on any live device connection.</p>
      </div>
      <div class="page-actions">
        <button
          v-if="undoCount > 0"
          class="secondary undo-btn"
          @click="undoLast"
          :title="`Undo last action (${undoCount} step${undoCount !== 1 ? 's' : ''} available) · Ctrl+Z`"
        >↩ {{ undoCount }}</button>
        <button class="secondary" @click="refreshRecordFiles" :disabled="loadingList">
          {{ loadingList ? 'Refreshing...' : '↻ Refresh List' }}
        </button>
        <button class="secondary" @click="loadConfig" :disabled="loadingConfig" title="Load a previously saved viewer config (TOML)">
          {{ loadingConfig ? 'Loading…' : '↑ Load Config' }}
        </button>
        <button class="secondary" @click="saveConfig" :disabled="savingConfig || loadedSignals.length === 0" title="Save current signals, view and zoom to a TOML file">
          {{ savingConfig ? 'Saving…' : '↓ Save Config' }}
        </button>
        <button @click="browseMf4File" :disabled="openingFile !== '' || loadingSignal !== ''">Open MF4…</button>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>

    <div class="layout" :style="layoutGridStyle">
      <!-- ── Side panels wrapper ────────────────────────────────────────── -->
      <div class="side-panels">
      <!-- ── File list panel ─────────────────────────────────────────────── -->
      <section class="panel file-panel" :class="{ 'panel-collapsed': !filesPanelOpen }">
        <template v-if="filesPanelOpen">
          <div class="panel-header">
            <div>
              <h2>Recorded Files</h2>
              <div class="panel-meta">{{ recordDirectory || 'records/' }}</div>
            </div>
            <div class="panel-header-actions">
              <span class="badge">{{ recordFiles.length }}</span>
              <button class="icon-btn" @click="filesPanelOpen = false" title="Collapse panel">◀</button>
            </div>
          </div>

          <div v-if="!directoryExists" class="empty-state">
            Directory not found yet. Set a recording folder in Settings or open an MF4 file manually.
          </div>
          <div v-else-if="loadingList" class="empty-state">Loading MF4 files…</div>
          <div v-else-if="recordFiles.length === 0" class="empty-state">No MF4 files found in the configured recording directory.</div>
          <div v-else-if="visibleFiles.length === 0" class="empty-state">
            All files hidden. <button class="link-btn" @click="hiddenFilePaths.clear()">Show all</button>
          </div>

          <div v-else class="file-list">
            <div
              v-for="file in visibleFiles"
              :key="file.path"
              class="file-row"
              :class="{ active: activeFile?.path === file.path }"
              @click="openMf4File(file.path)"
            >
              <span class="file-main">
                <span class="file-name">{{ file.name }}</span>
                <span class="file-row-right">
                  <span class="file-size">{{ formatBytes(file.size_bytes) }}</span>
                  <button class="file-remove-btn" @click.stop="hideFile(file.path)" title="Remove from list">✕</button>
                </span>
              </span>
              <span class="file-path">{{ file.path }}</span>
              <span class="file-offset-row" @click.stop>
                <label class="file-offset-label" :title="`Time offset for ${file.name}: shifts all channels from this file along X axis`">
                  <span class="file-offset-lbl-text">⏱ Offset</span>
                  <input
                    type="number"
                    class="file-offset-input"
                    :value="fileTimeOffsets[file.path] ?? 0"
                    step="0.001"
                    placeholder="0.000"
                    @change="setFileOffset(file.path, $event.target.value)"
                    @keydown.enter="setFileOffset(file.path, $event.target.value)"
                  />
                  <span class="file-offset-unit">s</span>
                </label>
              </span>
            </div>
          </div>
        </template>

        <div v-else class="panel-strip" @click="openFilesPanel()" title="Recorded Files — click to expand">
          <span class="panel-strip-icon">▤</span>
        </div>
      </section>

      <!-- ── Channel list panel ──────────────────────────────────────────── -->
      <section class="panel channel-panel" :class="{ 'panel-collapsed': !channelsPanelOpen }">
        <template v-if="channelsPanelOpen">
          <div class="panel-header">
            <div>
              <h2>Channels</h2>
              <div class="panel-meta">
                {{ activeFile ? `${activeFile.name} · ${activeChannels.length} channel(s)` : 'Open an MF4 file to browse channels' }}
              </div>
            </div>
            <div class="panel-header-actions">
              <span v-if="openingFile" class="badge loading">Loading…</span>
              <button class="icon-btn" @click="channelsPanelOpen = false" title="Collapse panel">◀</button>
            </div>
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
              :class="{ 'channel-row-dragging': draggingChannel && draggingChannel.channel === channel }"
              @dblclick="addSignal(channel)"
              @mousedown="onChannelMouseDown(channel, $event)"
            >
              <span class="channel-name">
                {{ channel }}
                <span v-if="activeVtabMaps[channel]" class="enum-badge" title="Enum channel (Value-to-Text)">ENUM</span>
              </span>
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
        </template>

        <div v-else class="panel-strip" @click="openChannelsPanel()" title="Channels — click to expand">
          <span class="panel-strip-icon">≋</span>
        </div>
      </section>
      </div><!-- /.side-panels -->

      <section class="panel chart-panel">
        <div class="panel-header">
          <div>
            <h2>Signal Graph</h2>
            <div class="panel-meta">{{ loadedSignals.length }} loaded signal(s)</div>
          </div>
          <div class="graph-header-right">
            <button
              class="cursor-toggle-btn"
              :class="{ active: mf4CursorVisible }"
              @click="mf4CursorVisible = !mf4CursorVisible"
              title="Toggle time cursor"
            >
              {{ mf4CursorVisible ? '🕹 Cursor ON' : '🕹 Cursor OFF' }}
            </button>
            <button v-if="loadedSignals.length > 0" class="secondary sm" @click="clearSignals">Clear All</button>
          </div>
        </div>

        <div v-if="loadedSignals.length === 0" class="empty-state chart-empty">
          Add one or more channels to render their recorded waveform here.<br>
          <span class="hint-text">Tip: drag a channel from the list and drop it onto an existing chart to overlay signals.</span>
        </div>
        <template v-else>
          <div
            v-for="group in chartGroups"
            :key="group.id"
            class="chart-group-wrap"
            :class="{ 'chart-drag-over': dragOverGroupId === group.id }"
            :data-group-id="group.id"
          >
            <SignalChart
              :signals="group.signals"
              :ref="el => registerChartRef(group.id, el)"
              :historyStore="historyStore"
              :running="false"
              :startEpoch="null"
              :gridColor="chartGridColor"
              :tickColor="chartTickColor"
              :vtabMaps="signalVtabMaps"
              :xMin="viewStart"
              :xMax="viewEnd"
              :showWindowSlider="false"
              :showCursor="mf4CursorVisible"
              :cursorTime="sharedCursorTime"
              :fileTimeOffsets="fileTimeOffsets"
              @cursor-move="sharedCursorTime = $event"
              @color-change="onSignalColorChange"
              @zoom-x="applyChartZoom"
              @zoom-y="onChartZoomY(group.id, $event)"
              @close="removeGroup(group.id)"
            />
          </div>

          <!-- Two-cursor time range selector -->
          <div class="trange-panel">
            <div class="trange-header">
              <span class="trange-title">⏱ Time Range</span>
              <div class="trange-inputs">
                <label class="trange-input-group">
                  <span>Start</span>
                  <input
                    type="number"
                    class="trange-input"
                    v-model.number="viewStartInput"
                    :min="dataTimeMin"
                    :max="viewEnd - 0.001"
                    step="0.001"
                    @change="applyStartInput"
                    @keydown.enter="applyStartInput"
                  />
                  <span>s</span>
                </label>
                <label class="trange-input-group">
                  <span>End</span>
                  <input
                    type="number"
                    class="trange-input"
                    v-model.number="viewEndInput"
                    :min="viewStart + 0.001"
                    :max="dataTimeMax"
                    step="0.001"
                    @change="applyEndInput"
                    @keydown.enter="applyEndInput"
                  />
                  <span>s</span>
                </label>
              </div>
              <button class="secondary sm" @click="resetView" :disabled="isFullView">Reset</button>
            </div>

            <div class="trange-track-wrap">
              <div class="trange-track" ref="tRangeTrackEl">
                <div
                  class="trange-selection"
                  :style="[selectionStyle, selectionPanStyle]"
                  :title="isPannable ? 'Drag to pan the time window' : ''"
                  @mousedown.prevent.stop="startPan($event)"
                ></div>
                <div
                  class="trange-handle"
                  :style="startHandleStyle"
                  @mousedown.prevent="startDrag('start', $event)"
                  title="Drag to set start time"
                ><span class="trange-handle-grip">⋮</span></div>
                <div
                  class="trange-handle"
                  :style="endHandleStyle"
                  @mousedown.prevent="startDrag('end', $event)"
                  title="Drag to set end time"
                ><span class="trange-handle-grip">⋮</span></div>
              </div>
              <div class="trange-axis">
                <span>{{ dataTimeMin.toFixed(2) }}s</span>
                <span>{{ dataTimeMid.toFixed(2) }}s</span>
                <span>{{ dataTimeMax.toFixed(2) }}s</span>
              </div>
            </div>
          </div>

          <div class="stats-card">
            <table class="stats-table">
              <thead>
                <tr>
                  <th></th>
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
                  <td>
                    <button class="stats-remove-btn" @click="removeSignal(row.key)" title="Remove signal">✕</button>
                  </td>
                  <td>
                    <span class="stats-signal-swatch" :style="{ background: row.color }"></span>
                    {{ row.name }}
                  </td>
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

  <!-- ── Leave warning dialog ────────────────────────────────────────────── -->
  <Teleport to="body">
    <div v-if="showLeaveWarning" class="confirm-overlay" @click.self="cancelLeave">
      <div class="confirm-dialog">
        <div class="confirm-title">⚠ Unsaved State</div>
        <div class="confirm-body">
          You have <strong>{{ loadedSignals.length }} signal(s)</strong> loaded.
          Navigating away will clear them — they won't be restored when you return to MF4 Viewer.
        </div>
        <div class="confirm-actions">
          <button class="secondary" @click="cancelLeave">Stay</button>
          <button class="danger" @click="confirmLeave">Leave anyway</button>
        </div>
      </div>
    </div>
  </Teleport>

  <!-- Drag ghost: floating label following the cursor during channel drag -->
  <Teleport to="body">
    <div
      v-if="dragGhost"
      class="channel-drag-ghost"
      :style="{ left: dragGhost.x + 'px', top: dragGhost.y + 'px' }"
    >
      📈 {{ dragGhost.channel }}
    </div>
  </Teleport>
</template>

<script setup>
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import { open, save as dialogSave } from '@tauri-apps/plugin-dialog'

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
const activeVtabMaps = ref({})   // { channelName: Array<{key, text}> }
const channelSearch = ref('')
const loadingList = ref(false)
const openingFile = ref('')
const loadingSignal = ref('')
const msg = ref('')
const msgErr = ref(false)
// ── Chart groups ── each group holds one or more signals rendered in one chart ─
const chartGroups = ref([])   // [{ id: string, signals: [sigObj, ...] }]
const loadedSignals = computed(() => chartGroups.value.flatMap(g => g.signals))
const historyStore = new Map()
const sharedCursorTime = ref(null)
const mf4CursorVisible = ref(false)

// ── Drag-and-drop state ───────────────────────────────────────────────────────
const draggingChannel = ref(null)   // { channel, file } | null
const dragOverGroupId = ref(null)   // group.id being hovered | null

// ── Undo queue (up to 10 user actions) ───────────────────────────────────────
// Action shapes:
//   { type: 'zoom-x',        prevStart, prevEnd }
//   { type: 'zoom-y',        groupId, prevMin, prevMax }
//   { type: 'add-signal',    key, groupId }
//   { type: 'remove-signal', key, sigObj, groupId, insertIndex }
//   { type: 'remove-group',  group, groupIndex }
//   { type: 'clear-all',     groups }   — groups is a snapshot; historyStore data retained
//
// remove-signal, remove-group, and clear-all preserve historyStore data for undo.
// Data is cleaned up when the entry is evicted from the 10-slot queue.
const undoQueue = []   // plain array — not reactive, mutations trigger no re-render
const undoCount = ref(0)   // reactive mirror of undoQueue.length for UI

function pushUndo(action) {
  if (undoQueue.length >= 10) {
    const evicted = undoQueue.shift()
    // Free historyStore data retained for remove/clear actions once they leave the queue
    if (evicted.type === 'remove-signal') {
      if (!chartGroups.value.some(g => g.signals.some(s => s.key === evicted.key))) {
        historyStore.delete(evicted.key)
      }
    } else if (evicted.type === 'remove-group') {
      for (const sig of evicted.group.signals) {
        if (!chartGroups.value.some(g => g.signals.some(s => s.key === sig.key))) {
          historyStore.delete(sig.key)
        }
      }
    } else if (evicted.type === 'clear-all') {
      for (const group of evicted.groups) {
        for (const sig of group.signals) {
          if (!chartGroups.value.some(g => g.signals.some(s => s.key === sig.key))) {
            historyStore.delete(sig.key)
          }
        }
      }
    }
  }
  undoQueue.push(action)
  undoCount.value = undoQueue.length
}

// ── Chart component refs (keyed by group.id) for Y-zoom undo ─────────────────
const chartRefs = new Map()

function registerChartRef(groupId, el) {
  if (el) chartRefs.set(groupId, el)
  else chartRefs.delete(groupId)
}

// ── Panel collapse state ──────────────────────────────────────────────────────
const filesPanelOpen = ref(true)
const channelsPanelOpen = ref(true)

const layoutGridStyle = computed(() => {
  if (filesPanelOpen.value) {
    return { gridTemplateColumns: `minmax(240px, 280px) minmax(420px, 1fr)` }
  } else if (channelsPanelOpen.value) {
    return { gridTemplateColumns: `minmax(260px, 340px) minmax(420px, 1fr)` }
  } else {
    return { gridTemplateColumns: `52px minmax(420px, 1fr)` }
  }
})

function openFilesPanel() {
  channelsPanelOpen.value = false
  filesPanelOpen.value = true
}

function openChannelsPanel() {
  filesPanelOpen.value = false
  channelsPanelOpen.value = true
}

// ── Navigation leave guard ────────────────────────────────────────────────────
const router = useRouter()
const showLeaveWarning = ref(false)
const pendingNavigateTo = ref(null)
const leaveConfirmed = ref(false)

onBeforeRouteLeave((to) => {
  if (loadedSignals.value.length > 0 && !leaveConfirmed.value) {
    showLeaveWarning.value = true
    pendingNavigateTo.value = to.fullPath
    return false
  }
  leaveConfirmed.value = false
})

function confirmLeave() {
  showLeaveWarning.value = false
  leaveConfirmed.value = true
  router.push(pendingNavigateTo.value)
}

function cancelLeave() {
  showLeaveWarning.value = false
  pendingNavigateTo.value = null
}

// ── Config save / load ────────────────────────────────────────────────────────
const savingConfig = ref(false)
const loadingConfig = ref(false)

async function saveConfig() {
  if (loadedSignals.value.length === 0) {
    setMsg('Nothing to save — add some signals first.', true)
    return
  }
  const filePath = await dialogSave({
    title: 'Save MF4 Viewer Config',
    defaultPath: 'mf4_viewer.toml',
    filters: [{ name: 'TOML Config', extensions: ['toml'] }],
  })
  if (!filePath) return

  savingConfig.value = true
  try {
    const groups = chartGroups.value.map(g => {
      const chartRef = chartRefs.get(g.id)
      const yZoom = chartRef?.getYZoom() ?? null
      return {
        id: g.id,
        signals: g.signals.map(s => ({
          file_path: s.path,
          channel: s.name,
          color: s.color,
        })),
        y_zoom: yZoom,  // { min, max } or null
      }
    })
    const config = {
      view: {
        start: viewStart.value,
        end: viewEnd.value,
        cursor_visible: mf4CursorVisible.value,
        cursor_time: sharedCursorTime.value ?? null,
        active_file_path: activeFile.value?.path ?? null,
        files_panel_open: filesPanelOpen.value,
        channels_panel_open: channelsPanelOpen.value,
      },
      groups,
      file_offsets: { ...fileTimeOffsets },
    }
    await invoke('mf4_save_config', { path: filePath, config })
    setMsg(`Config saved to ${filePath}`)
  } catch (e) {
    setMsg(String(e), true)
  } finally {
    savingConfig.value = false
  }
}

async function loadConfig() {
  const filePath = await open({
    title: 'Load MF4 Viewer Config',
    filters: [{ name: 'TOML Config', extensions: ['toml'] }],
    multiple: false,
    directory: false,
  })
  if (!filePath) return

  loadingConfig.value = true
  try {
    const config = await invoke('mf4_load_config', { path: filePath })

    // Open all unique MF4 files first — register them in the files panel
    const uniquePaths = [...new Set(config.groups.flatMap(g => g.signals.map(s => s.file_path)))]
    const openedFileMap = new Map()  // path → mf4_open_file response
    for (const p of uniquePaths) {
      try {
        const response = await invoke('mf4_open_file', { mf4Path: p })
        upsertRecordFile(response.file)
        openedFileMap.set(p, response)
      } catch {
        // File may be missing — we'll skip those signals below
      }
    }

    // Restore active file / channels panel
    const savedActivePath = config.view.active_file_path ?? null
    const activeResponse = savedActivePath
      ? (openedFileMap.get(savedActivePath) ?? openedFileMap.values().next().value ?? null)
      : (openedFileMap.values().next().value ?? null)
    if (activeResponse) {
      activeFile.value = activeResponse.file
      activeChannels.value = activeResponse.channels || []
      activeVtabMaps.value = activeResponse.vtab_maps || {}
      channelSearch.value = ''
    }

    // Restore panel open state
    filesPanelOpen.value = config.view.files_panel_open ?? true
    channelsPanelOpen.value = config.view.channels_panel_open ?? false
    // Mutual-exclusion: only one can be open at a time
    if (filesPanelOpen.value && channelsPanelOpen.value) {
      channelsPanelOpen.value = false
    }

    // Rebuild chart groups from config
    chartGroups.value = []
    for (const g of config.groups) {
      const signals = []
      for (const s of g.signals) {
        const key = signalKey(s.file_path, s.channel)
        try {
          const [dataBytes, timeBytes] = await Promise.all([
            invoke('mf4_get_channel_data', { mf4Path: s.file_path, channelName: s.channel }),
            invoke('mf4_get_channel_time', { mf4Path: s.file_path, channelName: s.channel }),
          ])
          const values = toFloat64Array(dataBytes)
          const times = toFloat64Array(timeBytes)
          if (times.length !== values.length) continue
          historyStore.set(key, { times, values })
          const fileName = s.file_path.split(/[\\/]/).pop() || s.file_path
          signals.push({
            key,
            name: s.channel,
            device_name: fileName,
            color: s.color,
            path: s.file_path,
            vtab_map: null,
          })
        } catch {
          // Signal unavailable (file missing or channel gone) — skip silently
        }
      }
      if (signals.length > 0) {
        chartGroups.value = [...chartGroups.value, { id: g.id, signals }]
      }
    }

    // Restore view range
    // Restore file time offsets before recomputing bounds
    Object.keys(fileTimeOffsets).forEach(k => delete fileTimeOffsets[k])
    if (config.file_offsets) {
      Object.assign(fileTimeOffsets, config.file_offsets)
    }

    viewStart.value = config.view.start
    viewEnd.value = config.view.end
    viewStartInput.value = Number(config.view.start.toFixed(3))
    viewEndInput.value = Number(config.view.end.toFixed(3))
    mf4CursorVisible.value = config.view.cursor_visible
    sharedCursorTime.value = config.view.cursor_time ?? null

    // Restore Y zoom after chart refs are mounted (next tick)
    const yZoomPending = config.groups
      .filter(g => g.y_zoom)
      .map(g => ({ id: g.id, min: g.y_zoom.min, max: g.y_zoom.max }))
    if (yZoomPending.length > 0) {
      setTimeout(() => {
        for (const { id, min, max } of yZoomPending) {
          const ref = chartRefs.get(id)
          if (ref) ref.setYZoom(min, max)
        }
      }, 100)
    }

    const total = config.groups.reduce((n, g) => n + g.signals.length, 0)
    const loaded = loadedSignals.value.length
    setMsg(`Config loaded: ${loaded} of ${total} signal(s) restored.`)
  } catch (e) {
    setMsg(String(e), true)
  } finally {
    loadingConfig.value = false
  }
}

// ── Per-file time offsets ─────────────────────────────────────────────────────
const fileTimeOffsets = reactive({})  // { [filePath]: offsetSeconds }
const dataTimeMin = ref(0)
const dataTimeMax = ref(1)
const viewStart = ref(0)
const viewEnd = ref(1)
const viewStartInput = ref(0)
const viewEndInput = ref(1)
const tRangeTrackEl = ref(null)
const isDraggingPan = ref(false)

const dataTimeMid = computed(() => (dataTimeMin.value + dataTimeMax.value) / 2)

const isFullView = computed(() =>
  Math.abs(viewStart.value - dataTimeMin.value) < 0.001 &&
  Math.abs(viewEnd.value - dataTimeMax.value) < 0.001
)

/** True when the view is narrower than the full data range — panning is meaningful */
const isPannable = computed(() => {
  const dataSpan = dataTimeMax.value - dataTimeMin.value
  if (dataSpan < 1e-10) return false
  const viewSpan = viewEnd.value - viewStart.value
  return viewSpan < dataSpan - 1e-10
})

const selectionPanStyle = computed(() => {
  if (!isPannable.value) return {}
  return { cursor: isDraggingPan.value ? 'grabbing' : 'grab' }
})

function timeToPct(t) {
  const span = dataTimeMax.value - dataTimeMin.value
  if (span < 1e-10) return 0
  return Math.max(0, Math.min(1, (t - dataTimeMin.value) / span))
}

function pctToTime(pct) {
  return dataTimeMin.value + pct * (dataTimeMax.value - dataTimeMin.value)
}

const startHandleStyle = computed(() => ({ left: `${timeToPct(viewStart.value) * 100}%` }))
const endHandleStyle = computed(() => ({ left: `${timeToPct(viewEnd.value) * 100}%` }))
const selectionStyle = computed(() => {
  const s = timeToPct(viewStart.value)
  const e = timeToPct(viewEnd.value)
  return { left: `${s * 100}%`, width: `${(e - s) * 100}%` }
})

function recomputeTimeBounds() {
  let min = Infinity
  let max = -Infinity
  for (const sig of loadedSignals.value) {
    const h = historyStore.get(sig.key)
    if (h && h.times.length > 0) {
      const offset = fileTimeOffsets[sig.path] ?? 0
      const tFirst = h.times[0] + offset
      const tLast = h.times[h.times.length - 1] + offset
      if (tFirst < min) min = tFirst
      if (tLast > max) max = tLast
    }
  }
  const newMin = Number.isFinite(min) ? min : 0
  const newMax = Number.isFinite(max) ? max : 1
  dataTimeMin.value = newMin
  dataTimeMax.value = newMax
  // Expand view to include any new data beyond current range
  if (viewStart.value > newMin) {
    viewStart.value = newMin
    viewStartInput.value = Number(newMin.toFixed(3))
  }
  if (viewEnd.value < newMax) {
    viewEnd.value = newMax
    viewEndInput.value = Number(newMax.toFixed(3))
  }
}

function resetView() {
  viewStart.value = dataTimeMin.value
  viewEnd.value = dataTimeMax.value
  viewStartInput.value = Number(dataTimeMin.value.toFixed(3))
  viewEndInput.value = Number(dataTimeMax.value.toFixed(3))
}

function applyStartInput() {
  const v = Math.max(dataTimeMin.value, Math.min(viewEnd.value - 0.001, Number(viewStartInput.value)))
  viewStart.value = v
  viewStartInput.value = Number(v.toFixed(3))
}

function applyEndInput() {
  const v = Math.min(dataTimeMax.value, Math.max(viewStart.value + 0.001, Number(viewEndInput.value)))
  viewEnd.value = v
  viewEndInput.value = Number(v.toFixed(3))
}

function startDrag(side, event) {
  event.preventDefault()
  const track = tRangeTrackEl.value
  if (!track) return

  const prevStart = viewStart.value
  const prevEnd = viewEnd.value
  let hasMoved = false

  function onMove(e) {
    hasMoved = true
    const rect = track.getBoundingClientRect()
    const pct = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width))
    const t = pctToTime(pct)
    if (side === 'start') {
      viewStart.value = Math.max(dataTimeMin.value, Math.min(viewEnd.value - 0.001, t))
      viewStartInput.value = Number(viewStart.value.toFixed(3))
    } else {
      viewEnd.value = Math.min(dataTimeMax.value, Math.max(viewStart.value + 0.001, t))
      viewEndInput.value = Number(viewEnd.value.toFixed(3))
    }
  }

  function onUp() {
    document.removeEventListener('mousemove', onMove)
    document.removeEventListener('mouseup', onUp)
    if (hasMoved) pushUndo({ type: 'zoom-x', prevStart, prevEnd })
  }

  document.addEventListener('mousemove', onMove)
  document.addEventListener('mouseup', onUp)
}

function startPan(event) {
  if (!isPannable.value) return
  event.preventDefault()

  const track = tRangeTrackEl.value
  if (!track) return

  isDraggingPan.value = true
  document.body.style.cursor = 'grabbing'

  const prevStart = viewStart.value
  const prevEnd = viewEnd.value
  const startX = event.clientX
  const startViewStart = viewStart.value
  const startViewEnd = viewEnd.value
  const dataSpan = dataTimeMax.value - dataTimeMin.value
  const viewSpan = startViewEnd - startViewStart
  let hasMoved = false

  function onMove(e) {
    hasMoved = true
    const rect = track.getBoundingClientRect()
    const dt = (e.clientX - startX) / rect.width * dataSpan

    let newStart = startViewStart + dt
    let newEnd = startViewEnd + dt

    // Clamp to data bounds, preserving span width
    if (newStart < dataTimeMin.value) {
      newStart = dataTimeMin.value
      newEnd = newStart + viewSpan
    }
    if (newEnd > dataTimeMax.value) {
      newEnd = dataTimeMax.value
      newStart = newEnd - viewSpan
    }

    viewStart.value = newStart
    viewEnd.value = newEnd
    viewStartInput.value = Number(newStart.toFixed(3))
    viewEndInput.value = Number(newEnd.toFixed(3))
  }

  function onUp() {
    isDraggingPan.value = false
    document.body.style.cursor = ''
    document.removeEventListener('mousemove', onMove)
    document.removeEventListener('mouseup', onUp)
    if (hasMoved) pushUndo({ type: 'zoom-x', prevStart, prevEnd })
  }

  document.addEventListener('mousemove', onMove)
  document.addEventListener('mouseup', onUp)
}

const loadedSignalKeys = computed(() => new Set(loadedSignals.value.map(signal => signal.key)))

// ── Hidden files (session-local; Refresh resets) ──────────────────────────────
const hiddenFilePaths = reactive(new Set())
const visibleFiles = computed(() => recordFiles.value.filter(f => !hiddenFilePaths.has(f.path)))

function hideFile(path) {
  hiddenFilePaths.add(path)
}

/** vtabMaps keyed by signal key for SignalChart — { [signalKey]: Array<{key, text}> } */
const signalVtabMaps = computed(() => {
  const result = {}
  for (const sig of loadedSignals.value) {
    if (sig.vtab_map && sig.vtab_map.length > 0) {
      result[sig.key] = sig.vtab_map
    }
  }
  return result
})

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
      color: signal.color ?? '#00cc44',
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

  const vtab = signal.vtab_map
  const vtabText = (raw) => {
    if (!vtab || vtab.length === 0) return null
    const entry = vtab.find(e => Math.abs(e.key - raw) < 0.5)
    return entry ? entry.text : null
  }
  const fmtEnum = (raw) => {
    const text = vtabText(raw)
    return text != null ? `${text} (${raw.toFixed(0)})` : formatValue(raw)
  }

  return {
    key: signal.key,
    name: signal.name,
    color: signal.color ?? '#00cc44',
    file: signal.device_name,
    samples: count.toLocaleString(),
    min: vtab ? fmtEnum(min) : formatValue(min),
    max: vtab ? fmtEnum(max) : formatValue(max),
    latest: vtab ? fmtEnum(values[values.length - 1]) : formatValue(values[values.length - 1]),
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

function setFileOffset(filePath, rawValue) {
  const offset = parseFloat(rawValue)
  if (!Number.isFinite(offset)) return
  // Find the max raw time for this file across all loaded signals from it
  let maxRawTime = 0
  for (const sig of loadedSignals.value) {
    if (sig.path !== filePath) continue
    const h = historyStore.get(sig.key)
    if (h && h.times.length > 0) {
      const last = h.times[h.times.length - 1]
      if (last > maxRawTime) maxRawTime = last
    }
  }
  // Negative offset must not push the last sample below t=0
  const clamped = maxRawTime > 0 ? Math.max(-maxRawTime, offset) : offset
  if (clamped === 0) {
    delete fileTimeOffsets[filePath]
  } else {
    fileTimeOffsets[filePath] = clamped
  }
  recomputeTimeBounds()
}


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
  hiddenFilePaths.clear()
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
    activeVtabMaps.value = response.vtab_maps || {}
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

async function addSignal(channelName, targetGroupId = null) {
  if (!activeFile.value) return
  const key = signalKey(activeFile.value.path, channelName)
  if (loadedSignalKeys.value.has(key)) {
    setMsg(`'${channelName}' is already loaded`, false)
    return
  }

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
    const vtabEntries = activeVtabMaps.value[channelName] || null
    const sigObj = {
      key,
      name: channelName,
      device_name: activeFile.value.name,
      color: pickColor(loadedSignals.value.length),
      path: activeFile.value.path,
      vtab_map: vtabEntries,
    }

    if (targetGroupId !== null) {
      const group = chartGroups.value.find(g => g.id === targetGroupId)
      if (group) {
        group.signals = [...group.signals, sigObj]
        pushUndo({ type: 'add-signal', key, groupId: targetGroupId })
        return
      }
    }
    // No target group → create a new chart
    const newGroupId = crypto.randomUUID()
    chartGroups.value = [...chartGroups.value, { id: newGroupId, signals: [sigObj] }]
    pushUndo({ type: 'add-signal', key, groupId: newGroupId })
  } catch (error) {
    setMsg(String(error), true)
  } finally {
    loadingSignal.value = ''
  }
}

function removeSignal(key) {
  // Capture position before removal so it can be restored by undo
  let groupId = null
  let insertIndex = 0
  let sigObj = null
  for (const group of chartGroups.value) {
    const idx = group.signals.findIndex(s => s.key === key)
    if (idx !== -1) {
      groupId = group.id
      insertIndex = idx
      sigObj = group.signals[idx]
      break
    }
  }
  // Remove from groups; keep historyStore data alive for undo
  chartGroups.value = chartGroups.value
    .map(g => ({ ...g, signals: g.signals.filter(s => s.key !== key) }))
    .filter(g => g.signals.length > 0)
  if (sigObj) pushUndo({ type: 'remove-signal', key, sigObj, groupId, insertIndex })
}

function removeGroup(groupId) {
  const groupIndex = chartGroups.value.findIndex(g => g.id === groupId)
  const group = groupIndex !== -1 ? chartGroups.value[groupIndex] : null
  // Remove from view; keep historyStore data alive for undo
  chartGroups.value = chartGroups.value.filter(g => g.id !== groupId)
  // Prune stale zoom-y undo entries for the deleted chart
  for (let i = undoQueue.length - 1; i >= 0; i--) {
    if (undoQueue[i].type === 'zoom-y' && undoQueue[i].groupId === groupId) {
      undoQueue.splice(i, 1)
    }
  }
  if (group) {
    pushUndo({ type: 'remove-group', group: { id: group.id, signals: [...group.signals] }, groupIndex })
  }
  undoCount.value = undoQueue.length
}

function onSignalColorChange({ key, color }) {
  const sig = loadedSignals.value.find(s => s.key === key)
  if (sig) sig.color = color
}

function onChartZoomY(groupId, { prevMin, prevMax }) {
  pushUndo({ type: 'zoom-y', groupId, prevMin, prevMax })
}

function undoLast() {
  const action = undoQueue.pop()
  undoCount.value = undoQueue.length
  if (!action) return
  if (action.type === 'zoom-x') {
    viewStart.value = action.prevStart
    viewEnd.value = action.prevEnd
    viewStartInput.value = Number(action.prevStart.toFixed(4))
    viewEndInput.value = Number(action.prevEnd.toFixed(4))
  } else if (action.type === 'zoom-y') {
    const chart = chartRefs.get(action.groupId)
    if (chart) chart.setYZoom(action.prevMin, action.prevMax)
  } else if (action.type === 'add-signal') {
    historyStore.delete(action.key)
    chartGroups.value = chartGroups.value
      .map(g => ({ ...g, signals: g.signals.filter(s => s.key !== action.key) }))
      .filter(g => g.signals.length > 0)
  } else if (action.type === 'remove-signal') {
    const group = chartGroups.value.find(g => g.id === action.groupId)
    if (group) {
      const signals = [...group.signals]
      signals.splice(action.insertIndex, 0, action.sigObj)
      group.signals = signals
    } else {
      // Original group was auto-removed when it became empty — recreate it
      chartGroups.value = [...chartGroups.value, { id: action.groupId, signals: [action.sigObj] }]
    }
  } else if (action.type === 'remove-group') {
    const groups = [...chartGroups.value]
    groups.splice(action.groupIndex, 0, action.group)
    chartGroups.value = groups
  } else if (action.type === 'clear-all') {
    chartGroups.value = action.groups
  }
}

function applyChartZoom({ xMin, xMax }) {
  const prevStart = viewStart.value
  const prevEnd = viewEnd.value
  viewStart.value = Math.max(dataTimeMin.value ?? xMin, xMin)
  viewEnd.value = Math.min(dataTimeMax.value ?? xMax, xMax)
  viewStartInput.value = Number(viewStart.value.toFixed(4))
  viewEndInput.value = Number(viewEnd.value.toFixed(4))
  pushUndo({ type: 'zoom-x', prevStart, prevEnd })
}

function clearSignals() {
  if (chartGroups.value.length === 0) return
  // Snapshot all groups before clearing; keep historyStore data alive for undo
  const snapshot = chartGroups.value.map(g => ({ id: g.id, signals: [...g.signals] }))
  chartGroups.value = []
  pushUndo({ type: 'clear-all', groups: snapshot })
}

// ── Mouse-based drag handlers (replaces unreliable HTML5 drag API in Tauri) ───
const dragGhost = ref(null)   // { channel, x, y } | null — ghost follows cursor
let _mouseMoveHandler = null
let _mouseUpHandler = null

function onChannelMouseDown(channel, e) {
  if (e.button !== 0) return   // left button only
  e.preventDefault()           // prevent text selection during drag

  const startX = e.clientX
  const startY = e.clientY
  let isDragging = false

  _mouseMoveHandler = (e) => {
    const dx = e.clientX - startX
    const dy = e.clientY - startY
    if (!isDragging && Math.sqrt(dx * dx + dy * dy) > 6) {
      isDragging = true
      draggingChannel.value = { channel, file: activeFile.value }
    }
    if (!isDragging) return

    dragGhost.value = { channel, x: e.clientX + 14, y: e.clientY - 14 }

    // Identify which chart-group-wrap (if any) is under the cursor
    // pointer-events:none on ghost ensures we can hit-test what's below it
    const els = document.elementsFromPoint(e.clientX, e.clientY)
    const wrap = els.find(el => el.dataset && el.dataset.groupId)
    dragOverGroupId.value = wrap ? wrap.dataset.groupId : null
  }

  _mouseUpHandler = async (e) => {
    document.removeEventListener('mousemove', _mouseMoveHandler)
    document.removeEventListener('mouseup', _mouseUpHandler)
    _mouseMoveHandler = null
    _mouseUpHandler = null

    const drag = draggingChannel.value
    const targetGroupId = dragOverGroupId.value
    dragGhost.value = null
    draggingChannel.value = null
    dragOverGroupId.value = null

    if (!isDragging || !drag || !targetGroupId) return
    await onDropOnGroup(targetGroupId, drag)
  }

  document.addEventListener('mousemove', _mouseMoveHandler)
  document.addEventListener('mouseup', _mouseUpHandler)
}

async function onDropOnGroup(groupId, drag) {
  const { channel, file } = drag
  if (!file) return
  const key = signalKey(file.path, channel)
  if (loadedSignalKeys.value.has(key)) {
    setMsg(`'${channel}' is already loaded`, false)
    return
  }

  loadingSignal.value = key
  try {
    const [rawValues, rawTimes] = await Promise.all([
      invoke('mf4_get_channel_data', { mf4Path: file.path, channelName: channel }),
      invoke('mf4_get_channel_time', { mf4Path: file.path, channelName: channel }),
    ])
    const values = toFloat64Array(rawValues)
    const times = toFloat64Array(rawTimes)
    if (values.length === 0 || times.length === 0) throw new Error(`Channel '${channel}' returned no samples`)
    if (values.length !== times.length) throw new Error(`Channel '${channel}' data/time length mismatch`)

    historyStore.set(key, { times, values })
    const vtabEntries = activeVtabMaps.value[channel] || null
    const sigObj = {
      key,
      name: channel,
      device_name: file.name,
      color: pickColor(loadedSignals.value.length),
      path: file.path,
      vtab_map: vtabEntries,
    }
    const group = chartGroups.value.find(g => g.id === groupId)
    if (group) {
      group.signals = [...group.signals, sigObj]
      pushUndo({ type: 'add-signal', key, groupId })
    }
  } catch (error) {
    setMsg(String(error), true)
  } finally {
    loadingSignal.value = ''
  }
}

watch(() => mf4Dir.value, () => {
  refreshRecordFiles()
})

watch(loadedSignals, () => {
  recomputeTimeBounds()
}, { deep: true })

// When cursor is toggled ON for the first time, place it at the midpoint of the view
watch(mf4CursorVisible, (visible) => {
  if (visible && sharedCursorTime.value === null) {
    const mid = ((viewStart.value ?? 0) + (viewEnd.value ?? 1)) / 2
    sharedCursorTime.value = mid
  }
})

onMounted(() => {
  refreshRecordFiles()
  window.addEventListener('keydown', onUndoKey)
})

onUnmounted(() => {
  window.removeEventListener('keydown', onUndoKey)
})

function onUndoKey(e) {
  if ((e.ctrlKey || e.metaKey) && e.key === 'z') {
    e.preventDefault()
    undoLast()
  }
}
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
  align-items: center;
}

.undo-btn {
  font-variant-numeric: tabular-nums;
  min-width: 52px;
}

.layout {
  display: grid;
  gap: 14px;
  align-items: start;
}

/* ── Side panels wrapper ─────────────────────────────────────────── */
.side-panels {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

/* Override global panel min-height for panels inside the side wrapper */
.side-panels .panel {
  min-height: 0;
}

/* An expanded side panel gets the full tall height */
.side-panels .file-panel:not(.panel-collapsed),
.side-panels .channel-panel:not(.panel-collapsed) {
  flex: 1 1 auto;
  min-height: 620px;
}

/* Collapsed panel inside side-panels: compact horizontal bar */
.side-panels .panel-collapsed {
  flex: 0 0 auto;
  min-height: 0;
  padding: 0;
  cursor: pointer;
  overflow: hidden;
}

/* Strip inside side-panels: horizontal bar, icon centred, no rotated text */
.side-panels .panel-strip {
  flex-direction: row;
  justify-content: center;
  align-items: center;
  height: 44px;
  min-height: 44px;
  padding: 0;
  gap: 0;
}

.side-panels .panel-strip-icon {
  font-size: 1.25rem;
  color: var(--exp-fg-dim, #556644);
}

.side-panels .panel-strip:hover .panel-strip-icon {
  color: var(--exp-fg-bright, #00ff66);
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

/* Collapsed sidebar strip — global defaults (overridden inside .side-panels) */
.panel-collapsed {
  padding: 0;
  min-height: 200px;
  cursor: pointer;
  overflow: hidden;
}

.panel-strip {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: flex-start;
  gap: 12px;
  height: 100%;
  min-height: 200px;
  padding: 14px 0;
  transition: background 0.15s;
}

.panel-strip:hover {
  background: var(--exp-hover, #003311);
}

.panel-strip-label {
  writing-mode: vertical-rl;
  text-orientation: mixed;
  transform: rotate(180deg);
  color: var(--exp-fg, #00cc44);
  font-size: 0.8rem;
  font-weight: 700;
  letter-spacing: 1px;
  user-select: none;
  white-space: nowrap;
}

.panel-strip:hover .panel-strip-label {
  color: var(--exp-fg-bright, #00ff66);
}

.panel-strip-icon {
  font-size: 0.75rem;
  color: var(--exp-fg-dim, #556644);
}

.panel-strip:hover .panel-strip-icon {
  color: var(--exp-fg-bright, #00ff66);
}

/* Header actions row (badge + collapse button) */
.panel-header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.icon-btn {
  background: transparent;
  border: 1px solid var(--exp-border, #003311);
  color: var(--exp-fg-dim, #556644);
  border-radius: 3px;
  padding: 2px 7px;
  cursor: pointer;
  font-size: 0.72rem;
  line-height: 1.4;
}

.icon-btn:hover {
  color: var(--exp-fg-bright, #00ff66);
  border-color: var(--exp-fg, #00cc44);
  background: var(--exp-hover, #003311);
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
  cursor: pointer;
}

.file-row.active {
  border-color: var(--exp-fg-bright, #00ff66);
  background: var(--exp-hover, #003311);
}

.file-row-right {
  display: flex;
  align-items: center;
  gap: 6px;
}

.file-remove-btn {
  border: none;
  background: transparent;
  color: var(--exp-fg-dim3, #335522);
  cursor: pointer;
  padding: 0 3px;
  font-size: 0.78rem;
  line-height: 1;
}

.file-remove-btn:hover {
  color: #ff6666;
}

.link-btn {
  background: none;
  border: none;
  color: var(--exp-fg, #00cc44);
  cursor: pointer;
  padding: 0;
  font-size: inherit;
  text-decoration: underline;
}

.link-btn:hover {
  color: var(--exp-fg-bright, #00ff66);
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

.file-offset-row {
  display: flex;
  align-items: center;
  padding: 3px 0 1px;
  border-top: 1px solid var(--exp-border, #003311);
  margin-top: 3px;
}

.file-offset-label {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 0.78rem;
  color: var(--exp-fg-dim, #556644);
  width: 100%;
}

.file-offset-lbl-text {
  flex-shrink: 0;
}

.file-offset-input {
  width: 80px;
  background: var(--exp-bg2, #060606);
  color: var(--exp-fg, #00cc44);
  border: 1px solid var(--exp-border, #003311);
  border-radius: 3px;
  padding: 2px 5px;
  font-family: 'Consolas', 'Courier New', monospace;
  font-size: 0.78rem;
  outline: none;
}

.file-offset-input:focus {
  border-color: var(--exp-fg-bright, #00ff66);
}

.file-offset-unit {
  flex-shrink: 0;
  color: var(--exp-fg-dim, #556644);
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
  cursor: grab;
  user-select: none;
}

.channel-row:active,
.channel-row-dragging {
  cursor: grabbing;
  opacity: 0.7;
}

/* Floating ghost label that follows the cursor during a drag */
.channel-drag-ghost {
  position: fixed;
  pointer-events: none;
  z-index: 9999;
  background: rgba(0, 30, 10, 0.92);
  border: 1px solid rgba(0, 220, 80, 0.6);
  border-radius: 5px;
  padding: 5px 10px;
  color: #00ff66;
  font-size: 12px;
  white-space: nowrap;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.6);
  transform: translate(0, -50%);
}

/* Wrapper for each chart group — enables native drag events */
.chart-group-wrap {
  position: relative;
}

/* Highlight when a dragged channel is hovering over a chart */
.chart-drag-over {
  outline: 2px dashed rgba(0, 220, 80, 0.6);
  outline-offset: 2px;
  background: rgba(0, 220, 80, 0.04);
}

.channel-name {
  color: var(--exp-fg, #00cc44);
  word-break: break-word;
}

.enum-badge {
  display: inline-block;
  margin-left: 6px;
  padding: 1px 5px;
  font-size: 0.68rem;
  font-weight: 700;
  border-radius: 3px;
  background: rgba(255, 204, 0, 0.12);
  color: #ffcc00;
  border: 1px solid rgba(255, 204, 0, 0.3);
  vertical-align: middle;
  letter-spacing: 0.5px;
}

.chart-panel {
  min-height: 620px;
}

.chart-empty {
  min-height: 180px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-direction: column;
  gap: 8px;
  text-align: center;
}

.hint-text {
  font-size: 11px;
  color: var(--exp-fg-dim3, #556644);
  font-style: italic;
}

.graph-header-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.cursor-toggle-btn {
  padding: 4px 10px;
  font-size: 11px;
  border-radius: 4px;
  border: 1px solid var(--exp-border2, #002211);
  background: var(--exp-bg2, #060606);
  color: var(--exp-fg-dim3, #667744);
  cursor: pointer;
  white-space: nowrap;
  transition: color 0.15s, border-color 0.15s;
}

.cursor-toggle-btn:hover,
.cursor-toggle-btn.active {
  color: #ffcc00;
  border-color: rgba(255, 204, 0, 0.5);
  background: rgba(255, 204, 0, 0.05);
}

.stats-remove-btn {
  background: none;
  border: none;
  color: var(--exp-fg-dim3, #335522);
  cursor: pointer;
  font-size: 12px;
  padding: 0 4px;
  line-height: 1;
}

.stats-remove-btn:hover {
  color: #ff6666;
}

.stats-signal-swatch {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-right: 5px;
  vertical-align: middle;
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

/* ── Time range selector ────────────────────────────────────────────────────── */
.trange-panel {
  margin-top: 10px;
  border: 1px solid var(--exp-border, #003311);
  border-radius: 6px;
  background: var(--exp-bg2, #060606);
  padding: 10px 12px 12px;
}

.trange-header {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
  flex-wrap: wrap;
}

.trange-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--exp-fg-bright, #00ff66);
  letter-spacing: 0.5px;
  white-space: nowrap;
  flex: 1;
}

.trange-inputs {
  display: flex;
  align-items: center;
  gap: 14px;
  flex-wrap: wrap;
}

.trange-input-group {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  color: var(--exp-fg-dim, #556644);
}

.trange-input {
  width: 74px;
  background: var(--exp-bg, #000);
  border: 1px solid var(--exp-border, #003311);
  color: var(--exp-fg-bright, #00ff66);
  border-radius: 3px;
  padding: 3px 6px;
  font-size: 11px;
  font-family: 'Consolas', 'Courier New', monospace;
  text-align: right;
  outline: none;
  -moz-appearance: textfield;
}
.trange-input::-webkit-inner-spin-button,
.trange-input::-webkit-outer-spin-button { opacity: 0.3; }
.trange-input:focus {
  border-color: var(--exp-fg-bright, #00ff66);
}

.trange-track-wrap {
  position: relative;
  padding-bottom: 20px;
}

.trange-track {
  position: relative;
  height: 22px;
  background: var(--exp-bg, #000);
  border: 1px solid var(--exp-border, #003311);
  border-radius: 4px;
  overflow: visible;
}

.trange-selection {
  position: absolute;
  top: 0;
  height: 100%;
  background: rgba(0, 220, 80, 0.12);
  border-left: 2px solid var(--exp-fg, #00cc44);
  border-right: 2px solid var(--exp-fg, #00cc44);
}

.trange-handle {
  position: absolute;
  top: -5px;
  width: 14px;
  height: calc(100% + 10px);
  margin-left: -7px;
  background: var(--exp-fg, #00cc44);
  border-radius: 3px;
  cursor: grab;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 2;
  transition: background 0.12s, box-shadow 0.12s;
  box-shadow: 0 0 4px rgba(0, 204, 68, 0.4);
}
.trange-handle:hover {
  background: var(--exp-fg-bright, #00ff66);
  box-shadow: 0 0 8px rgba(0, 255, 102, 0.5);
}
.trange-handle:active {
  cursor: grabbing;
  background: var(--exp-fg-bright, #00ff66);
}

.trange-handle-grip {
  font-size: 13px;
  color: var(--exp-bg, #000);
  user-select: none;
  line-height: 1;
  font-weight: 900;
  letter-spacing: -1px;
}

.trange-axis {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  display: flex;
  justify-content: space-between;
  font-size: 10px;
  color: var(--exp-fg-dim3, #335522);
  font-variant-numeric: tabular-nums;
  padding: 0 2px;
}

@media (max-width: 1400px) {
  .layout {
    grid-template-columns: 1fr !important;
  }

  .side-panels {
    flex-direction: row;
    flex-wrap: wrap;
  }

  .side-panels .file-panel:not(.panel-collapsed),
  .side-panels .channel-panel:not(.panel-collapsed) {
    min-height: auto;
  }

  .panel {
    min-height: auto;
  }
}

/* ── Leave warning dialog ────────────────────────────────────────────────── */
.confirm-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.78);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 200;
}

.confirm-dialog {
  background: var(--exp-bg3, #0d0d0d);
  border: 1px solid var(--exp-border, #003311);
  border-radius: 6px;
  padding: 24px;
  max-width: 440px;
  width: 90vw;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.confirm-title {
  color: #ffcc00;
  font-size: 1.05rem;
  font-weight: 700;
}

.confirm-body {
  color: var(--exp-fg, #00cc44);
  font-size: 0.9rem;
  line-height: 1.6;
}

.confirm-body strong {
  color: var(--exp-fg-bright, #00ff66);
}

.confirm-actions {
  display: flex;
  gap: 10px;
  justify-content: flex-end;
}

button.danger {
  background: #3a0011;
  border: 1px solid #880033;
  color: #ff4466;
  padding: 5px 14px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.85rem;
}

button.danger:hover {
  background: #550022;
  border-color: #cc0044;
  color: #ff6688;
}
</style>
