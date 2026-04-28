<template>
  <div class="page">
    <h1>DAQ Measurements</h1>

    <div class="toolbar">
      <select v-model="selectedDevice" @change="onDeviceChange" class="sel">
        <option value="">Select device…</option>
        <option v-for="d in connectedDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
      </select>
      <span class="status-pill" :class="running ? 'running' : ''">{{ running ? '● Running' : '○ Stopped' }}</span>
      <span v-if="running" class="elapsed-txt">{{ elapsedStr }}</span>
      <button @click="start" :disabled="!selectedDevice || running || loading">▶ Start</button>
      <button @click="stop" :disabled="!running" class="warn">■ Stop</button>
      <button @click="clearSelection" :disabled="running">Clear</button>
    </div>

    <div class="split">
      <!-- Left: selected measurements list -->
      <div class="panel left-panel">
        <div class="panel-header">
          <h2>Measurements ({{ selectedMeasurements.length }})</h2>
          <button @click="openAddModal" class="sm">+ Add</button>
        </div>
        <div v-if="selectedMeasurements.length === 0" class="empty">No measurements selected.</div>
        <div v-for="m in selectedMeasurements" :key="m.idx" class="meas-item">
          <div class="meas-left">
            <span class="meas-name" :style="{ color: signalColor(m.name) }">{{ m.name }}</span>
            <span class="meas-meta">{{ m.data_type }} · ch{{ m.event_channel }}</span>
          </div>
          <span class="meas-live">{{ formatVal(liveData[m.name]) }}</span>
          <button class="sm danger" @click="removeMeasurement(m.idx)">✕</button>
        </div>
      </div>

      <!-- Right: live values + recording -->
      <div class="panel right-panel">
        <h2>Live Data</h2>
        <div v-if="Object.keys(liveData).length === 0" class="empty">
          {{ running ? 'Waiting for data…' : 'Start DAQ to see values' }}
        </div>
        <div class="live-list">
          <div v-for="(val, name) in liveData" :key="name" class="live-row">
            <span class="live-name" :style="{ color: signalColor(name) }">{{ name }}</span>
            <span class="live-val">{{ formatVal(val) }}</span>
          </div>
        </div>

        <div class="record-section">
          <div class="rec-row">
            <input v-model="csvFilename" class="inp" placeholder="filename.csv" style="flex:1" />
            <button @click="startRecording" :disabled="!running || recording">⏺ Record</button>
            <button @click="stopRecording" :disabled="!recording" class="warn sm">■ Stop</button>
          </div>
          <div class="rec-row">
            <button @click="downloadCSV" :disabled="!csvReady" style="margin-top:6px">⬇ Download CSV</button>
          </div>
        </div>
      </div>
    </div>

    <!-- Graph toggle -->
    <div class="graph-toggle-bar">
      <button @click="showGraph = !showGraph" :class="{ active: showGraph }">📊 Graph</button>
      <template v-if="showGraph">
        <label class="check-label">
          <input type="checkbox" v-model="chartAutoFollow" /> Auto-follow
        </label>
        <button @click="clearChart" class="sm warn">Clear</button>
      </template>
      <!-- Signal color legend -->
      <div class="legend" v-if="showGraph">
        <span v-for="m in selectedMeasurements" :key="m.name" class="legend-item">
          <span class="legend-dot" :style="{ background: signalColor(m.name) }"></span>
          {{ m.name }}
        </span>
      </div>
    </div>

    <div v-if="showGraph" class="graph-panel">
      <canvas ref="chartCanvas" class="chart-canvas"></canvas>
    </div>

    <!-- Add Measurements modal -->
    <div v-if="showAddModal" class="modal-overlay" @click.self="showAddModal = false">
      <div class="modal modal-wide">
        <h2>Add Measurements</h2>
        <input v-model="addSearch" placeholder="Search…" class="inp full" style="margin-bottom:10px" />
        <div class="add-table-wrap">
          <table class="add-table">
            <thead>
              <tr><th></th><th>Name</th><th>Data Type</th><th>Unit</th><th>Channel</th></tr>
            </thead>
            <tbody>
              <tr v-for="m in filteredAllMeasurements" :key="m.idx"
                :class="{ 'in-sel': pendingIdxs.has(m.idx) }"
                @click="togglePending(m.idx)">
                <td><input type="checkbox" :checked="pendingIdxs.has(m.idx)"
                  @change="togglePending(m.idx)" @click.stop /></td>
                <td class="add-name">{{ m.name }}</td>
                <td>{{ m.data_type }}</td>
                <td>{{ m.unit || '—' }}</td>
                <td>{{ m.event_channel }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div class="modal-actions">
          <button @click="applyAdd" :disabled="applyingAdd">{{ applyingAdd ? 'Applying…' : 'Apply' }}</button>
          <button @click="showAddModal = false" class="warn">Cancel</button>
        </div>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

const COLORS = ['#00ff66','#ff9933','#5599ff','#ff44aa','#ffcc00','#44ffcc','#ff4444','#aa44ff','#88ff00','#00ccff']
const MAX_POINTS = 600
const TIME_WINDOW = 30

const connectedDevices = ref([])
const selectedDevice = ref('')
const allMeasurements = ref([])
const liveData = ref({})
const chartData = ref({})
const colorMap = ref({})
const showGraph = ref(false)
const chartAutoFollow = ref(true)
const running = ref(false)
const recording = ref(false)
const loading = ref(false)
const csvFilename = ref('daq_data.csv')
const csvReady = ref(false)
const msg = ref('')
const msgErr = ref(false)
const elapsedSecs = ref(0)
const showAddModal = ref(false)
const addSearch = ref('')
const pendingIdxs = ref(new Set())
const applyingAdd = ref(false)
const chartCanvas = ref(null)

let unlisten = null
let elapsedTimer = null
let rafId = null
let alive = true

const selectedMeasurements = computed(() => allMeasurements.value.filter(m => m.selected))

const filteredAllMeasurements = computed(() => {
  if (!addSearch.value) return allMeasurements.value
  const q = addSearch.value.toLowerCase()
  return allMeasurements.value.filter(m => m.name.toLowerCase().includes(q))
})

const elapsedStr = computed(() => {
  const s = elapsedSecs.value
  return `${String(Math.floor(s / 60)).padStart(2,'0')}:${String(s % 60).padStart(2,'0')}`
})

function signalColor(name) {
  if (!colorMap.value[name]) {
    const idx = Object.keys(colorMap.value).length % COLORS.length
    colorMap.value = { ...colorMap.value, [name]: COLORS[idx] }
  }
  return colorMap.value[name]
}

function formatVal(v) {
  if (v === undefined || v === null) return '—'
  return typeof v === 'number' ? v.toFixed(4) : String(v)
}

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 5000)
}

async function loadDevices() {
  try {
    const r = await invoke('list_devices')
    connectedDevices.value = (r.devices ?? []).filter(d => d.connected)
    if (connectedDevices.value.length === 1 && !selectedDevice.value) {
      selectedDevice.value = connectedDevices.value[0].id
      await loadMeasurements()
    }
  } catch { }
}

async function onDeviceChange() {
  allMeasurements.value = []; liveData.value = {}; chartData.value = {}
  await loadMeasurements()
}

async function loadMeasurements() {
  if (!selectedDevice.value) return
  try {
    const r = await invoke('daq_available')
    allMeasurements.value = (r.measurements ?? [])
      .filter(m => m.device_id === selectedDevice.value)
      .map(m => ({
        ...m,
        data_type: m.data_type ?? m.type,
        event_channel: m.event_channel ?? '—'
      }))
    try {
      const st = await invoke('daq_status')
      const deviceStatus = (st.devices ?? []).find(d => d.device_id === selectedDevice.value)
      running.value = !!deviceStatus?.running
    } catch { }
  } catch (e) { setMsg(String(e), true) }
}

async function start() {
  loading.value = true
  try {
    await invoke('daq_start', { deviceId: selectedDevice.value })
    running.value = true; startElapsed()
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = false }
}

async function stop() {
  try {
    await invoke('daq_stop', { deviceId: selectedDevice.value })
    running.value = false; stopElapsed()
  } catch (e) { setMsg(String(e), true) }
}

async function clearSelection() {
  try {
    await invoke('daq_clear', { deviceId: selectedDevice.value })
    await loadMeasurements()
  } catch (e) { setMsg(String(e), true) }
}

async function removeMeasurement(idx) {
  try {
    await invoke('daq_toggle', { deviceId: selectedDevice.value, idx })
    await loadMeasurements()
  } catch (e) { setMsg(String(e), true) }
}

async function startRecording() {
  try {
    await invoke('daq_start_recording', { deviceId: selectedDevice.value, config: { filename: csvFilename.value } })
    recording.value = true
  } catch (e) { setMsg(String(e), true) }
}

async function stopRecording() {
  try {
    await invoke('daq_stop_recording', { deviceId: selectedDevice.value })
    recording.value = false; csvReady.value = true
  } catch (e) { setMsg(String(e), true) }
}

async function downloadCSV() {
  try {
    const csv = await invoke('daq_download_csv', { deviceId: selectedDevice.value })
    const blob = new Blob([csv], { type: 'text/csv' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url; a.download = csvFilename.value; a.click()
    URL.revokeObjectURL(url)
  } catch (e) { setMsg(String(e), true) }
}

function startElapsed() {
  elapsedSecs.value = 0; stopElapsed()
  elapsedTimer = setInterval(() => elapsedSecs.value++, 1000)
}
function stopElapsed() {
  if (elapsedTimer) { clearInterval(elapsedTimer); elapsedTimer = null }
}

function openAddModal() {
  pendingIdxs.value = new Set(selectedMeasurements.value.map(m => m.idx))
  addSearch.value = ''
  showAddModal.value = true
}

function togglePending(idx) {
  const ns = new Set(pendingIdxs.value)
  if (ns.has(idx)) ns.delete(idx); else ns.add(idx)
  pendingIdxs.value = ns
}

async function applyAdd() {
  if (!selectedDevice.value) return
  applyingAdd.value = true
  try {
    await invoke('daq_set_selection', { deviceId: selectedDevice.value, indices: [...pendingIdxs.value] })
    await loadMeasurements()
    showAddModal.value = false
  } catch (e) { setMsg(String(e), true) }
  finally { applyingAdd.value = false }
}

function clearChart() { chartData.value = {} }

// --- Canvas chart ---
function drawChart() {
  if (!alive || !showGraph.value) return
  const canvas = chartCanvas.value
  if (!canvas) { rafId = requestAnimationFrame(drawChart); return }
  const parent = canvas.parentElement
  if (parent) canvas.width = parent.clientWidth || 600
  canvas.height = 280
  const ctx = canvas.getContext('2d')
  const W = canvas.width, H = canvas.height
  const PAD = { l: 60, r: 16, t: 14, b: 28 }
  const pW = W - PAD.l - PAD.r
  const pH = H - PAD.t - PAD.b

  ctx.fillStyle = '#050505'
  ctx.fillRect(0, 0, W, H)

  const now = Date.now() / 1000
  const signals = Object.keys(chartData.value).filter(n => chartData.value[n].length > 0)

  if (signals.length === 0) {
    ctx.fillStyle = '#333'; ctx.font = '12px Consolas'; ctx.textAlign = 'center'
    ctx.fillText('No data', W / 2, H / 2)
    rafId = requestAnimationFrame(drawChart); return
  }

  let tMin = Infinity, tMax = -Infinity, vMin = Infinity, vMax = -Infinity
  for (const name of signals) {
    for (const pt of chartData.value[name]) {
      if (pt.t < tMin) tMin = pt.t; if (pt.t > tMax) tMax = pt.t
      if (pt.v < vMin) vMin = pt.v; if (pt.v > vMax) vMax = pt.v
    }
  }
  if (chartAutoFollow.value) { tMax = now; tMin = now - TIME_WINDOW }
  if (tMax <= tMin) tMax = tMin + 1
  if (vMax === vMin) { vMin -= 1; vMax += 1 }
  const vRange = vMax - vMin
  vMin -= vRange * 0.05; vMax += vRange * 0.05

  const tx = t => PAD.l + ((t - tMin) / (tMax - tMin)) * pW
  const ty = v => PAD.t + (1 - (v - vMin) / (vMax - vMin)) * pH

  ctx.strokeStyle = '#111'; ctx.lineWidth = 1
  for (let i = 0; i <= 6; i++) {
    const x = PAD.l + (i / 6) * pW
    ctx.beginPath(); ctx.moveTo(x, PAD.t); ctx.lineTo(x, PAD.t + pH); ctx.stroke()
    const label = chartAutoFollow.value ? `-${Math.round(TIME_WINDOW - (i / 6) * TIME_WINDOW)}s` : (tMin + (i / 6) * (tMax - tMin)).toFixed(1)
    ctx.fillStyle = '#444'; ctx.font = '10px Consolas'; ctx.textAlign = 'center'
    ctx.fillText(label, x, H - PAD.b + 12)
  }
  for (let i = 0; i <= 5; i++) {
    const y = PAD.t + (i / 5) * pH
    ctx.beginPath(); ctx.moveTo(PAD.l, y); ctx.lineTo(PAD.l + pW, y); ctx.stroke()
    const v = vMax - (i / 5) * (vMax - vMin)
    ctx.fillStyle = '#444'; ctx.font = '10px Consolas'; ctx.textAlign = 'right'
    ctx.fillText(v.toFixed(2), PAD.l - 4, y + 3)
  }

  for (const name of signals) {
    const pts = chartData.value[name]
    const color = colorMap.value[name] || '#00ff66'
    ctx.strokeStyle = color; ctx.lineWidth = 1.5; ctx.beginPath()
    let first = true
    for (const pt of pts) {
      if (chartAutoFollow.value && (pt.t < tMin || pt.t > tMax)) continue
      const x = tx(pt.t), y = ty(pt.v)
      if (first) { ctx.moveTo(x, y); first = false } else ctx.lineTo(x, y)
    }
    ctx.stroke()
  }

  rafId = requestAnimationFrame(drawChart)
}

watch(showGraph, v => {
  if (v) nextTick(() => { rafId = requestAnimationFrame(drawChart) })
  else { if (rafId) { cancelAnimationFrame(rafId); rafId = null } }
})

onMounted(async () => {
  await loadDevices()
  unlisten = await listen('daq-data', event => {
    const p = event.payload
    if (!p || p.device_id !== selectedDevice.value) return
    if (p.values) {
      liveData.value = { ...p.values }
      const t = p.timestamp_s ?? (Date.now() / 1000)
      const newData = { ...chartData.value }
      for (const [name, val] of Object.entries(p.values)) {
        if (!newData[name]) newData[name] = []
        newData[name] = [...newData[name], { t, v: val }].slice(-MAX_POINTS)
        signalColor(name)
      }
      chartData.value = newData
    }
  })
})

onUnmounted(() => {
  alive = false
  if (unlisten) unlisten()
  stopElapsed()
  if (rafId) cancelAnimationFrame(rafId)
})
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 14px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 0.95rem; margin-bottom: 8px; }
.toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
.status-pill { font-size: 0.82rem; padding: 2px 8px; border: 1px solid #003311; }
.status-pill.running { color: #00ff66; border-color: #006622; background: #001a09; }
.elapsed-txt { font-family: Consolas, monospace; font-size: 0.9rem; color: #00cc44; }
.sel { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: Consolas, monospace; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: Consolas, monospace; font-size: 0.9rem; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
button.sm { padding: 2px 7px; font-size: 0.8rem; }
button.warn { border-color: #ffcc00; color: #ffcc00; }
button.warn:hover:not(:disabled) { background: #1a1100; }
button.danger { border-color: #ff4444; color: #ff4444; }
button.danger:hover:not(:disabled) { background: #1a0000; }
button.active { background: #003311; color: #00ff66; }
.split { display: flex; gap: 16px; margin-bottom: 12px; }
.panel { background: #0d0d0d; border: 1px solid #003311; padding: 12px; }
.left-panel { flex: 1; min-width: 0; }
.right-panel { flex: 1; min-width: 0; }
.panel-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px; }
.meas-item { display: flex; align-items: center; gap: 8px; padding: 5px 0; border-bottom: 1px solid #001a09; }
.meas-left { flex: 1; }
.meas-name { font-weight: bold; font-size: 0.9rem; }
.meas-meta { display: block; color: #555; font-size: 0.75rem; }
.meas-live { font-family: Consolas, monospace; font-size: 0.9rem; color: #00cc44; min-width: 80px; text-align: right; }
.live-list { max-height: 200px; overflow-y: auto; margin-bottom: 10px; }
.live-row { display: flex; justify-content: space-between; padding: 3px 0; border-bottom: 1px solid #001a09; }
.live-name { font-size: 0.88rem; font-weight: bold; }
.live-val { font-family: Consolas, monospace; font-size: 0.88rem; color: #00ff66; }
.record-section { border-top: 1px solid #003311; padding-top: 10px; }
.rec-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: Consolas, monospace; }
.inp.full { width: 100%; box-sizing: border-box; }
.graph-toggle-bar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 8px; }
.check-label { display: flex; align-items: center; gap: 4px; color: #006622; font-size: 0.85rem; cursor: pointer; }
.check-label input { accent-color: #00cc44; }
.legend { display: flex; flex-wrap: wrap; gap: 10px; margin-left: 8px; }
.legend-item { display: flex; align-items: center; gap: 4px; font-size: 0.8rem; color: #00cc44; }
.legend-dot { width: 10px; height: 10px; border-radius: 50%; }
.graph-panel { background: #050505; border: 1px solid #003311; }
.chart-canvas { display: block; width: 100%; }
.empty { color: #555; padding: 8px 0; }
.modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.75); display: flex; align-items: center; justify-content: center; z-index: 100; }
.modal { background: #0d0d0d; border: 1px solid #003311; padding: 20px; min-width: 420px; max-height: 85vh; overflow-y: auto; }
.modal.modal-wide { min-width: 620px; max-width: 90vw; }
.modal-actions { display: flex; gap: 8px; margin-top: 12px; }
.add-table-wrap { max-height: 360px; overflow-y: auto; border: 1px solid #003311; }
.add-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
.add-table th { color: #006622; text-align: left; padding: 5px 8px; border-bottom: 1px solid #003311; position: sticky; top: 0; background: #0d0d0d; }
.add-table td { padding: 4px 8px; border-bottom: 1px solid #001a09; }
.add-table tr { cursor: pointer; }
.add-table tr:hover td { background: #001a09; }
.add-table tr.in-sel td { background: #001a09; }
.add-table input[type=checkbox] { accent-color: #00cc44; }
.add-name { color: #00ff66; }
.msg { margin-top: 8px; font-size: 0.85rem; color: #00cc44; }
.msg.err { color: #ff4444; }
</style>
