<template>
  <div class="page">
    <h1>DAQ Measurements</h1>

    <div class="toolbar">
      <select v-model="selectedDevice" @change="loadMeasurements" class="sel">
        <option value="">Select device…</option>
        <option v-for="d in connectedDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
      </select>
      <span class="status-badge" :class="running ? 'running' : ''">{{ running ? '● Recording' : '○ Stopped' }}</span>
      <button @click="start" :disabled="!selectedDevice || running || loading">▶ Start</button>
      <button @click="stop" :disabled="!running" class="warn">■ Stop</button>
      <button @click="clear" :disabled="running">Clear Selection</button>
    </div>

    <div class="split">
      <!-- Available measurements -->
      <div class="panel">
        <h2>Available ({{ available.length }})</h2>
        <input v-model="search" placeholder="Search…" class="inp" />
        <div class="meas-list">
          <div v-for="(m, i) in filteredAvail" :key="i" class="meas-row" :class="{ selected: m.selected }" @click="toggle(m.idx)">
            <span class="meas-name">{{ m.name }}</span>
            <span class="meas-type">{{ m.data_type }}</span>
          </div>
        </div>
      </div>

      <!-- Live data -->
      <div class="panel">
        <h2>Live Values</h2>
        <div v-if="!running && !liveData" class="empty">Start DAQ to see live values</div>
        <div class="live-list">
          <div v-for="(val, name) in liveData" :key="name" class="live-row">
            <span class="live-name">{{ name }}</span>
            <span class="live-val">{{ typeof val === 'number' ? val.toFixed(4) : val }}</span>
          </div>
        </div>

        <div class="record-row">
          <input v-model="csvFilename" placeholder="output.csv" class="inp" />
          <button @click="startRecording" :disabled="!running || recording">● Record CSV</button>
          <button @click="stopRecording" :disabled="!recording" class="warn">■ Stop</button>
          <button @click="downloadCSV" :disabled="!csvReady">⬇ Download CSV</button>
        </div>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

const connectedDevices = ref([])
const selectedDevice = ref('')
const available = ref([])
const running = ref(false)
const recording = ref(false)
const loading = ref(false)
const search = ref('')
const liveData = ref(null)
const csvFilename = ref('daq_data.csv')
const csvReady = ref(false)
const msg = ref('')
const msgErr = ref(false)

let unlisten = null
let pollTimer = null

const filteredAvail = computed(() => {
  if (!search.value) return available.value
  return available.value.filter(m => m.name.toLowerCase().includes(search.value.toLowerCase()))
})

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 4000)
}

async function loadDevices() {
  const r = await invoke('list_devices')
  connectedDevices.value = r.devices.filter(d => d.connected)
  if (connectedDevices.value.length === 1 && !selectedDevice.value) {
    selectedDevice.value = connectedDevices.value[0].id
    await loadMeasurements()
  }
}

async function loadMeasurements() {
  if (!selectedDevice.value) return
  try {
    const r = await invoke('daq_available', { deviceId: selectedDevice.value })
    available.value = r.measurements
    const st = await invoke('daq_status', { deviceId: selectedDevice.value })
    running.value = st.running; recording.value = st.recording
  } catch (e) { setMsg(String(e), true) }
}

async function toggle(idx) {
  if (!selectedDevice.value) return
  try {
    await invoke('daq_toggle', { deviceId: selectedDevice.value, idx })
    await loadMeasurements()
  } catch (e) { setMsg(String(e), true) }
}

async function clear() {
  if (!selectedDevice.value) return
  try {
    await invoke('daq_clear', { deviceId: selectedDevice.value })
    await loadMeasurements()
  } catch (e) { setMsg(String(e), true) }
}

async function start() {
  loading.value = true
  try {
    await invoke('daq_start', { deviceId: selectedDevice.value, intervalMs: 100 })
    running.value = true
    startPolling()
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = false }
}

async function stop() {
  try {
    await invoke('daq_stop', { deviceId: selectedDevice.value })
    running.value = false; stopPolling()
  } catch (e) { setMsg(String(e), true) }
}

function startPolling() {
  pollTimer = setInterval(async () => {
    if (!running.value) return
    try {
      const r = await invoke('daq_latest', { deviceId: selectedDevice.value })
      if (r.sample) liveData.value = r.sample
    } catch { /* ignore */ }
  }, 200)
}
function stopPolling() { if (pollTimer) { clearInterval(pollTimer); pollTimer = null } }

async function startRecording() {
  try {
    await invoke('daq_start_recording', { deviceId: selectedDevice.value, filename: csvFilename.value })
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
    const r = await invoke('daq_download_csv', { deviceId: selectedDevice.value })
    const blob = new Blob([r.csv], { type: 'text/csv' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url; a.download = csvFilename.value; a.click()
    URL.revokeObjectURL(url)
  } catch (e) { setMsg(String(e), true) }
}

onMounted(async () => {
  await loadDevices()
  // Listen for pushed DAQ data events from backend
  unlisten = await listen('daq-data', (event) => {
    if (event.payload?.sample) liveData.value = event.payload.sample
    if (event.payload?.device_id === selectedDevice.value) running.value = event.payload.running ?? running.value
  })
})

onUnmounted(() => {
  if (unlisten) unlisten()
  stopPolling()
})
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 12px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 1rem; margin-bottom: 8px; }

.toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 16px; }
.sel { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: inherit; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
button.warn { border-color: #ffcc00; color: #ffcc00; }
.status-badge { font-size: 0.8rem; color: #444; padding: 3px 8px; border: 1px solid #222; }
.status-badge.running { color: #00ff66; border-color: #003311; animation: pulse 1.5s infinite; }
@keyframes pulse { 0%,100% { opacity:1 } 50% { opacity:0.5 } }

.split { display: flex; gap: 16px; margin-bottom: 16px; }
.panel { flex: 1; background: #0d0d0d; border: 1px solid #003311; padding: 12px; }

.meas-list { max-height: 300px; overflow-y: auto; margin-top: 8px; }
.meas-row { padding: 6px 8px; cursor: pointer; display: flex; justify-content: space-between; border-bottom: 1px solid #001a09; }
.meas-row:hover { background: #111; }
.meas-row.selected { background: #001a09; border-left: 2px solid #00ff66; }
.meas-name { color: #00ff66; }
.meas-type { color: #555; font-size: 0.8rem; }

.live-list { max-height: 260px; overflow-y: auto; margin-bottom: 12px; }
.live-row { display: flex; justify-content: space-between; padding: 4px 0; border-bottom: 1px solid #001a09; font-size: 0.9rem; }
.live-name { color: #00cc44; }
.live-val { color: #00ff66; font-weight: bold; }

.record-row { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-top: 8px; }
.empty { color: #555; padding: 16px 0; font-size: 0.9rem; }
.msg { margin-top: 8px; font-size: 0.85rem; color: #00cc44; }
.msg.err { color: #ff4444; }
</style>
