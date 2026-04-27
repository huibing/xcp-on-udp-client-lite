<template>
  <div class="page exp-page">
    <!-- Title bar -->
    <div class="title-bar">
      <h1>Experiment</h1>
      <select v-model="selectedDevice" @change="onDeviceChange" class="sel">
        <option value="">Select device…</option>
        <option v-for="d in allDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
      </select>
      <button @click="runFull"
        :disabled="!selectedDevice || running || !hexPath || !a2lPath || !dcmPath"
        class="run-btn">▶ Run Full</button>
      <span class="status-pill" :class="running ? 'active' : ''">{{ running ? '● Running' : '○ Idle' }}</span>
    </div>

    <div class="main-area">
      <!-- VSCode-like sidebar -->
      <div class="sidebar">
        <button v-for="tab in tabs" :key="tab.id" class="sb-btn"
          :class="{ active: activeTab === tab.id }"
          @click="activeTab = tab.id" :title="tab.label">{{ tab.icon }}</button>
        <div class="sb-sep"></div>
        <button class="sb-btn sb-run" @click="runFull"
          :disabled="!selectedDevice || running || !hexPath || !a2lPath || !dcmPath"
          title="Run All">▶</button>
      </div>

      <!-- Content area -->
      <div class="content-area">
        <div class="tab-bar">
          <button v-for="tab in tabs" :key="tab.id" class="tab-btn"
            :class="{ active: activeTab === tab.id }" @click="activeTab = tab.id">
            {{ tab.icon }} {{ tab.label }}
          </button>
        </div>

        <div class="tab-content">
          <!-- HEX tab -->
          <div v-show="activeTab === 'hex'" class="tab-pane">
            <h2>HEX File &amp; Checksum</h2>
            <div class="file-row">
              <span class="path-disp">{{ hexPath || '(no file selected)' }}</span>
              <button @click="browseHex">Browse…</button>
              <button @click="loadHex" :disabled="!selectedDevice || !hexPath || hexLoading">
                {{ hexLoading ? 'Loading…' : 'Load HEX' }}
              </button>
              <button @click="verifyChecksum" :disabled="!selectedDevice || !hexLoaded || verifyLoading">
                {{ verifyLoading ? 'Verifying…' : 'Verify Checksum' }}
              </button>
            </div>
            <div v-if="hexStatus" class="step-status" :class="hexStatus.ok ? 'ok' : 'fail'">{{ hexStatus.msg }}</div>
            <template v-if="checksumResults.length > 0">
              <div class="banner" :class="checksumPass ? 'pass' : 'fail'">
                {{ checksumPass ? '✓ ALL PASS' : '✗ MISMATCH' }}
              </div>
              <table class="res-table">
                <thead><tr><th>Name</th><th>Address</th><th>Expected</th><th>Actual</th><th>Match</th></tr></thead>
                <tbody>
                  <tr v-for="r in checksumResults" :key="r.name + r.address"
                    :class="r.match ? 'ok-row' : 'fail-row'">
                    <td>{{ r.name }}</td><td class="mono">{{ r.address }}</td>
                    <td class="mono">{{ r.expected }}</td><td class="mono">{{ r.actual }}</td>
                    <td>{{ r.match ? '✓' : '✗' }}</td>
                  </tr>
                </tbody>
              </table>
            </template>
          </div>

          <!-- A2L tab -->
          <div v-show="activeTab === 'a2l'" class="tab-pane">
            <h2>A2L File</h2>
            <div class="file-row">
              <span class="path-disp">{{ a2lPath || '(no file selected)' }}</span>
              <button @click="browseA2L">Browse…</button>
              <button @click="loadA2L" :disabled="!selectedDevice || !a2lPath || a2lLoading">
                {{ a2lLoading ? 'Loading…' : 'Load A2L' }}
              </button>
            </div>
            <div v-if="a2lStatus" class="step-status" :class="a2lStatus.ok ? 'ok' : 'fail'">{{ a2lStatus.msg }}</div>
          </div>

          <!-- DCM tab -->
          <div v-show="activeTab === 'dcm'" class="tab-pane">
            <h2>DCM File &amp; Write Parameters</h2>
            <div class="file-row">
              <span class="path-disp">{{ dcmPath || '(no file selected)' }}</span>
              <button @click="browseDCM">Browse…</button>
              <button @click="loadDCM" :disabled="!selectedDevice || !dcmPath || dcmLoading">
                {{ dcmLoading ? 'Loading…' : 'Load DCM' }}
              </button>
              <button @click="writeParams" :disabled="!selectedDevice || !dcmLoaded || writeLoading" class="warn">
                {{ writeLoading ? 'Writing…' : 'Write to ECU' }}
              </button>
            </div>
            <div v-if="dcmStatus" class="step-status" :class="dcmStatus.ok ? 'ok' : 'fail'">{{ dcmStatus.msg }}</div>
          </div>

          <!-- DAQ tab -->
          <div v-show="activeTab === 'daq'" class="tab-pane">
            <h2>DAQ</h2>
            <div class="row">
              <button @click="startDaq" :disabled="!selectedDevice || daqRunning || daqLoading">▶ Start DAQ</button>
              <button @click="stopDaq" :disabled="!daqRunning" class="warn">■ Stop DAQ</button>
              <span class="status-pill" :class="daqRunning ? 'active' : ''">
                {{ daqRunning ? '● Running' : '○ Stopped' }}
              </span>
            </div>
            <div v-if="daqStatus" class="step-status" :class="daqStatus.ok ? 'ok' : 'fail'" style="margin-top:8px">{{ daqStatus.msg }}</div>
            <div v-if="Object.keys(daqLiveData).length > 0" class="live-table-wrap">
              <table class="live-table">
                <thead><tr><th>Name</th><th>Value</th></tr></thead>
                <tbody>
                  <tr v-for="(val, name) in daqLiveData" :key="name">
                    <td class="lname">{{ name }}</td>
                    <td class="lval">{{ formatVal(val) }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>

          <!-- Results tab -->
          <div v-show="activeTab === 'results'" class="tab-pane">
            <h2>Step Summary</h2>
            <table class="res-table" v-if="stepResults.length > 0">
              <thead><tr><th>Step</th><th>Status</th><th>Detail</th></tr></thead>
              <tbody>
                <tr v-for="s in stepResults" :key="s.step" :class="s.ok ? 'ok-row' : 'fail-row'">
                  <td>{{ s.step }}</td>
                  <td>{{ s.ok ? '✓ OK' : '✗ FAIL' }}</td>
                  <td>{{ s.detail }}</td>
                </tr>
              </tbody>
            </table>
            <div v-else class="empty">No steps completed yet.</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Progress log (fixed height at bottom) -->
    <div class="progress-log">
      <div class="log-header">Progress Log <span class="log-count">({{ logEntries.length }})</span></div>
      <div class="log-entries" ref="logContainer">
        <div v-if="logEntries.length === 0" class="log-empty">No log entries</div>
        <div v-for="(entry, i) in logEntries" :key="i" class="log-entry" :class="entry.type">
          <span class="log-time">{{ entry.time }}</span>
          <span class="log-msg">{{ entry.message }}</span>
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

const tabs = [
  { id: 'hex',     icon: '🔍', label: 'HEX' },
  { id: 'a2l',     icon: '📄', label: 'A2L' },
  { id: 'dcm',     icon: '📋', label: 'DCM' },
  { id: 'daq',     icon: '📊', label: 'DAQ' },
  { id: 'results', icon: '📈', label: 'Results' },
]

const allDevices = ref([])
const selectedDevice = ref('')
const activeTab = ref('hex')
const running = ref(false)
const msg = ref('')
const msgErr = ref(false)
const logEntries = ref([])
const logContainer = ref(null)
const stepResults = ref([])

const hexPath = ref('')
const hexLoaded = ref(false)
const hexLoading = ref(false)
const hexStatus = ref(null)
const checksumResults = ref([])
const verifyLoading = ref(false)
const checksumPass = computed(() => checksumResults.value.length > 0 && checksumResults.value.every(r => r.match))

const a2lPath = ref('')
const a2lLoaded = ref(false)
const a2lLoading = ref(false)
const a2lStatus = ref(null)

const dcmPath = ref('')
const dcmLoaded = ref(false)
const dcmLoading = ref(false)
const dcmStatus = ref(null)
const writeLoading = ref(false)

const daqRunning = ref(false)
const daqLoading = ref(false)
const daqStatus = ref(null)
const daqLiveData = ref({})
let daqPollTimer = null

let unlistenProgress = null
let unlistenDone = null

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 5000)
}

function addLog(message, type = 'info') {
  const now = new Date()
  const time = [now.getHours(), now.getMinutes(), now.getSeconds()]
    .map(n => String(n).padStart(2, '0')).join(':')
  logEntries.value.unshift({ time, message, type })
  if (logEntries.value.length > 200) logEntries.value.pop()
}

function addStepResult(step, ok, detail) {
  const idx = stepResults.value.findIndex(s => s.step === step)
  const entry = { step, ok, detail: detail || '' }
  if (idx >= 0) stepResults.value[idx] = entry
  else stepResults.value.push(entry)
}

function formatVal(v) {
  if (v === undefined || v === null) return '—'
  return typeof v === 'number' ? v.toFixed(4) : String(v)
}

async function loadDevices() {
  try {
    const r = await invoke('exp_list_devices')
    allDevices.value = r.devices ?? []
  } catch (e) { setMsg(String(e), true) }
}

function onDeviceChange() {
  hexLoaded.value = false; a2lLoaded.value = false; dcmLoaded.value = false
  hexStatus.value = null; a2lStatus.value = null; dcmStatus.value = null; daqStatus.value = null
  checksumResults.value = []; stepResults.value = []; daqLiveData.value = {}
  daqRunning.value = false; stopDaqPoll()
}

async function browseHex() {
  try {
    const r = await invoke('open_file_dialog', {
      title: 'Select HEX File',
      filters: [{ name: 'HEX Files', extensions: ['hex', 'HEX'] }]
    })
    if (r.path) { hexPath.value = r.path; hexLoaded.value = false; hexStatus.value = null }
  } catch (e) { setMsg(String(e), true) }
}

async function loadHex() {
  hexLoading.value = true; hexStatus.value = null; checksumResults.value = []
  try {
    const r = await invoke('exp_load_hex', { deviceId: selectedDevice.value, filePath: hexPath.value })
    hexLoaded.value = r.ok
    hexStatus.value = { ok: r.ok, msg: r.ok ? '✓ HEX loaded successfully' : '✗ HEX load failed' }
    addLog(hexStatus.value.msg, r.ok ? 'success' : 'error')
    addStepResult('Load HEX', r.ok, hexPath.value.split('\\').pop() || hexPath.value.split('/').pop())
  } catch (e) {
    hexStatus.value = { ok: false, msg: String(e) }
    addLog(String(e), 'error')
  }
  finally { hexLoading.value = false }
}

async function verifyChecksum() {
  verifyLoading.value = true; checksumResults.value = []
  try {
    const r = await invoke('exp_verify_checksum', { deviceId: selectedDevice.value })
    checksumResults.value = r.results ?? []
    const pass = checksumResults.value.every(x => x.match)
    const passCount = checksumResults.value.filter(x => x.match).length
    addLog(`Checksum: ${pass ? 'ALL PASS' : 'MISMATCH'} (${passCount}/${checksumResults.value.length})`, pass ? 'success' : 'error')
    addStepResult('Verify Checksum', pass, `${passCount}/${checksumResults.value.length} pass`)
  } catch (e) { setMsg(String(e), true); addLog(String(e), 'error') }
  finally { verifyLoading.value = false }
}

async function browseA2L() {
  try {
    const r = await invoke('open_file_dialog', {
      title: 'Select A2L File',
      filters: [{ name: 'A2L Files', extensions: ['a2l', 'A2L'] }]
    })
    if (r.path) { a2lPath.value = r.path; a2lLoaded.value = false; a2lStatus.value = null }
  } catch (e) { setMsg(String(e), true) }
}

async function loadA2L() {
  a2lLoading.value = true; a2lStatus.value = null
  try {
    const r = await invoke('exp_load_a2l', { deviceId: selectedDevice.value, filePath: a2lPath.value })
    a2lLoaded.value = r.ok
    a2lStatus.value = { ok: r.ok, msg: r.ok ? '✓ A2L loaded successfully' : '✗ A2L load failed' }
    addLog(a2lStatus.value.msg, r.ok ? 'success' : 'error')
    addStepResult('Load A2L', r.ok, a2lPath.value.split('\\').pop() || a2lPath.value.split('/').pop())
  } catch (e) {
    a2lStatus.value = { ok: false, msg: String(e) }
    addLog(String(e), 'error')
  }
  finally { a2lLoading.value = false }
}

async function browseDCM() {
  try {
    const r = await invoke('open_file_dialog', {
      title: 'Select DCM File',
      filters: [{ name: 'DCM Files', extensions: ['dcm', 'DCM'] }]
    })
    if (r.path) { dcmPath.value = r.path; dcmLoaded.value = false; dcmStatus.value = null }
  } catch (e) { setMsg(String(e), true) }
}

async function loadDCM() {
  dcmLoading.value = true; dcmStatus.value = null
  try {
    const r = await invoke('exp_load_dcm', { deviceId: selectedDevice.value, filePath: dcmPath.value })
    dcmLoaded.value = r.ok
    dcmStatus.value = { ok: r.ok, msg: r.ok ? `✓ DCM loaded: ${r.param_count} params` : '✗ DCM load failed' }
    addLog(dcmStatus.value.msg, r.ok ? 'success' : 'error')
    addStepResult('Load DCM', r.ok, `${r.param_count ?? 0} params`)
  } catch (e) {
    dcmStatus.value = { ok: false, msg: String(e) }
    addLog(String(e), 'error')
  }
  finally { dcmLoading.value = false }
}

async function writeParams() {
  writeLoading.value = true
  try {
    const r = await invoke('exp_write_params', { deviceId: selectedDevice.value })
    const text = r.ok ? `✓ Written ${r.written ?? 0} params` : '✗ Write failed'
    dcmStatus.value = { ok: r.ok, msg: text }
    addLog(text, r.ok ? 'success' : 'error')
    addStepResult('Write Params', r.ok, `${r.written ?? 0} written`)
  } catch (e) {
    dcmStatus.value = { ok: false, msg: String(e) }
    addLog(String(e), 'error')
  }
  finally { writeLoading.value = false }
}

async function startDaq() {
  daqLoading.value = true
  try {
    const r = await invoke('exp_start_daq', { deviceId: selectedDevice.value })
    daqRunning.value = r.ok
    daqStatus.value = { ok: r.ok, msg: r.ok ? '● DAQ running' : '✗ DAQ start failed' }
    addLog(daqStatus.value.msg, r.ok ? 'success' : 'error')
    if (r.ok) startDaqPoll()
  } catch (e) {
    daqStatus.value = { ok: false, msg: String(e) }
    addLog(String(e), 'error')
  }
  finally { daqLoading.value = false }
}

async function stopDaq() {
  try {
    await invoke('exp_stop_daq', { deviceId: selectedDevice.value })
    daqRunning.value = false; stopDaqPoll()
    daqStatus.value = { ok: true, msg: '○ DAQ stopped' }
    addLog('DAQ stopped', 'info')
  } catch (e) { setMsg(String(e), true) }
}

function startDaqPoll() {
  stopDaqPoll()
  daqPollTimer = setInterval(async () => {
    if (!daqRunning.value) { stopDaqPoll(); return }
    try {
      const r = await invoke('daq_latest', { deviceId: selectedDevice.value })
      if (r.sample) daqLiveData.value = { ...r.sample }
    } catch { }
  }, 500)
}

function stopDaqPoll() {
  if (daqPollTimer) { clearInterval(daqPollTimer); daqPollTimer = null }
}

async function runFull() {
  if (!selectedDevice.value || !hexPath.value || !a2lPath.value || !dcmPath.value) return
  running.value = true; stepResults.value = []
  addLog('Starting full experiment run…', 'info')
  try {
    const r = await invoke('exp_run_full', {
      deviceId: selectedDevice.value,
      hexPath: hexPath.value,
      a2lPath: a2lPath.value,
      dcmPath: dcmPath.value
    })
    if (r && !r.ok) {
      addLog('Run Full returned error', 'error')
      running.value = false
    }
  } catch (e) {
    addLog(String(e), 'error')
    running.value = false
  }
}

onMounted(async () => {
  await loadDevices()
  unlistenProgress = await listen('exp-progress', event => {
    const p = event.payload
    if (p) addLog(`[${p.step || '?'}] ${p.message || ''}`, 'info')
  })
  unlistenDone = await listen('exp-done', event => {
    const p = event.payload
    running.value = false
    if (p) {
      addLog(p.message || (p.ok ? 'Done' : 'Failed'), p.ok ? 'success' : 'error')
      addStepResult('Run Full', p.ok, p.message || '')
      activeTab.value = 'results'
    }
  })
})

onUnmounted(() => {
  if (unlistenProgress) unlistenProgress()
  if (unlistenDone) unlistenDone()
  stopDaqPoll()
})
</script>

<style scoped>
.exp-page { display: flex; flex-direction: column; height: 100%; color: #00cc44; }
h1 { color: #00ff66; font-size: 1.3rem; margin: 0; }
h2 { color: #00cc44; font-size: 0.95rem; margin-bottom: 10px; }

.title-bar { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; padding-bottom: 10px; border-bottom: 1px solid #003311; margin-bottom: 0; }
.sel { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: Consolas, monospace; }
.run-btn { border-color: #00ff66; color: #00ff66; font-weight: bold; }
.status-pill { font-size: 0.82rem; padding: 2px 8px; border: 1px solid #003311; }
.status-pill.active { color: #00ff66; border-color: #006622; background: #001a09; }

.main-area { display: flex; flex: 1; min-height: 0; overflow: hidden; }

.sidebar { width: 44px; background: #080808; border-right: 1px solid #003311; display: flex; flex-direction: column; align-items: center; padding: 6px 0; gap: 2px; flex-shrink: 0; }
.sb-btn { width: 40px; height: 40px; background: transparent; border: none; color: #006622; cursor: pointer; font-size: 1rem; display: flex; align-items: center; justify-content: center; }
.sb-btn:hover { color: #00cc44; background: #0d0d0d; }
.sb-btn.active { color: #00ff66; border-left: 2px solid #00ff66; background: #0d0d0d; }
.sb-btn:disabled { opacity: 0.35; cursor: default; }
.sb-run { color: #00ff66; margin-top: auto; }
.sb-sep { flex: 1; }

.content-area { flex: 1; display: flex; flex-direction: column; min-width: 0; overflow: hidden; }

.tab-bar { display: flex; gap: 0; border-bottom: 1px solid #003311; background: #080808; overflow-x: auto; flex-shrink: 0; }
.tab-btn { background: transparent; border: none; border-bottom: 2px solid transparent; color: #555; padding: 6px 12px; cursor: pointer; font-family: Consolas, monospace; font-size: 0.82rem; white-space: nowrap; }
.tab-btn:hover { color: #00cc44; }
.tab-btn.active { color: #00ff66; border-bottom-color: #00ff66; background: #0a0a0a; }

.tab-content { flex: 1; overflow-y: auto; }
.tab-pane { padding: 12px; }

.file-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 10px; }
.path-disp { color: #006622; font-family: Consolas, monospace; font-size: 0.82rem; flex: 1; min-width: 80px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.step-status { padding: 6px 10px; border: 1px solid; font-size: 0.85rem; margin-bottom: 10px; }
.step-status.ok { color: #00ff66; border-color: #006622; background: #001a09; }
.step-status.fail { color: #ff4444; border-color: #660000; background: #1a0000; }
.banner { padding: 6px 12px; border: 1px solid; font-weight: bold; margin-bottom: 10px; }
.banner.pass { color: #00ff66; border-color: #006622; background: #001a09; }
.banner.fail { color: #ff4444; border-color: #660000; background: #1a0000; }
.res-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
.res-table th { color: #006622; text-align: left; padding: 5px 8px; border-bottom: 1px solid #003311; }
.res-table td { padding: 4px 8px; border-bottom: 1px solid #001a09; }
.ok-row td { color: #00cc44; }
.fail-row td { color: #ff4444; }
.mono { font-family: Consolas, monospace; }

.row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.live-table-wrap { margin-top: 12px; max-height: 250px; overflow-y: auto; }
.live-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
.live-table th { color: #006622; text-align: left; padding: 4px 8px; border-bottom: 1px solid #003311; position: sticky; top: 0; background: #0d0d0d; }
.live-table td { padding: 3px 8px; border-bottom: 1px solid #001a09; }
.lname { color: #00cc44; }
.lval { font-family: Consolas, monospace; color: #00ff66; text-align: right; }
.empty { color: #555; padding: 12px 0; }

.progress-log { height: 150px; border-top: 1px solid #003311; background: #080808; display: flex; flex-direction: column; flex-shrink: 0; }
.log-header { color: #006622; font-size: 0.75rem; padding: 4px 10px; border-bottom: 1px solid #001a09; flex-shrink: 0; }
.log-count { color: #444; }
.log-entries { flex: 1; overflow-y: auto; padding: 4px 0; }
.log-empty { color: #333; font-size: 0.8rem; padding: 6px 10px; }
.log-entry { display: flex; gap: 8px; padding: 2px 10px; font-size: 0.8rem; font-family: Consolas, monospace; }
.log-entry.info { color: #555; }
.log-entry.success { color: #00cc44; }
.log-entry.error { color: #ff4444; }
.log-entry.warn { color: #ffcc00; }
.log-time { color: #333; flex-shrink: 0; }
.log-msg { flex: 1; word-break: break-word; }

button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: Consolas, monospace; font-size: 0.9rem; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
button.warn { border-color: #ffcc00; color: #ffcc00; }
button.warn:hover:not(:disabled) { background: #1a1100; }
.msg { font-size: 0.85rem; color: #00cc44; padding: 4px 0; }
.msg.err { color: #ff4444; }
</style>
