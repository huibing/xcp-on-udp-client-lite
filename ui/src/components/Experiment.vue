<template>
  <div class="page">
    <h1>Experiment</h1>

    <div class="toolbar">
      <select v-model="selectedDevice" @change="loadVars" class="sel">
        <option value="">Select device…</option>
        <option v-for="d in connectedDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
      </select>
      <span class="status-badge" :class="expStatus?.active ? 'active' : ''">
        {{ expStatus?.active ? '● Active' : '○ Idle' }}
      </span>
      <button @click="startExp" :disabled="!selectedDevice || expStatus?.active">▶ Start</button>
      <button @click="stopExp" :disabled="!expStatus?.active" class="warn">■ Stop</button>
      <button @click="startRecording" :disabled="!expStatus?.active || recording">● Record</button>
      <button @click="stopRecording" :disabled="!recording" class="warn">■ Stop Rec</button>
    </div>

    <div v-if="selectedDevice" class="actions-row">
      <select v-model="targetPageMode" class="sel-sm">
        <option value="Working">Working Page</option>
        <option value="Reference">Reference Page</option>
      </select>
      <button @click="setPageMode">Set Page Mode</button>
      <button @click="runChecksum">Verify Checksum</button>
    </div>

    <div v-if="checksumResult" class="checksum-result" :class="checksumResult.all_match ? 'ok' : 'fail'">
      Checksum: {{ checksumResult.all_match ? '✓ Match' : '✗ Mismatch' }}
    </div>

    <!-- Variable list with change tracking -->
    <div class="var-section">
      <h2>Variables ({{ vars.length }})</h2>
      <input v-model="varSearch" placeholder="Search…" class="inp" />
      <div class="var-list">
        <div v-for="(v, i) in filteredVars" :key="v.name" class="var-row">
          <div class="var-main">
            <span class="var-name">{{ v.name }}</span>
            <span class="var-type">{{ v.type }}</span>
          </div>
          <div class="var-vals">
            <span class="orig-val" :title="'Original: ' + (originals[v.name] ?? '?')">
              orig: {{ originals[v.name] ?? '…' }}
            </span>
            <span class="curr-val">curr: {{ v.current_value ?? '—' }}</span>
            <span v-if="changedMap[v.name]" class="changed-badge">CHANGED</span>
          </div>
          <div class="var-actions">
            <button class="sm" @click="loadOriginal(v.name, i)">Snap</button>
            <button class="sm" @click="writeVar(v)">Write</button>
          </div>
        </div>
      </div>
    </div>

    <!-- Write dialog -->
    <div v-if="writeTarget" class="overlay">
      <div class="dialog">
        <h3>Write: {{ writeTarget.name }}</h3>
        <input v-model="writeValue" class="inp" @keyup.enter="confirmWrite" placeholder="New value" />
        <div class="row">
          <button @click="confirmWrite">Write</button>
          <button @click="writeTarget = null" class="warn">Cancel</button>
        </div>
        <div v-if="writeMsg" class="msg" :class="writeErr ? 'err' : ''">{{ writeMsg }}</div>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, reactive, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const connectedDevices = ref([])
const selectedDevice = ref('')
const vars = ref([])
const varSearch = ref('')
const expStatus = ref(null)
const recording = ref(false)
const checksumResult = ref(null)
const targetPageMode = ref('Working')
const originals = reactive({})
const changedMap = reactive({})
const writeTarget = ref(null)
const writeValue = ref('')
const writeMsg = ref('')
const writeErr = ref(false)
const msg = ref('')
const msgErr = ref(false)

const filteredVars = computed(() => {
  if (!varSearch.value) return vars.value
  return vars.value.filter(v => v.name.toLowerCase().includes(varSearch.value.toLowerCase()))
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
    await loadVars()
  }
}

async function loadVars() {
  if (!selectedDevice.value) return
  try {
    const r = await invoke('experiment_variables', { deviceId: selectedDevice.value })
    vars.value = r.variables ?? r
    await loadStatus()
  } catch (e) { setMsg(String(e), true) }
}

async function loadStatus() {
  try {
    expStatus.value = await invoke('experiment_status')
    recording.value = expStatus.value?.recording ?? false
  } catch { /* ignore */ }
}

async function startExp() {
  try { await invoke('experiment_start', { deviceId: selectedDevice.value }); await loadStatus() }
  catch (e) { setMsg(String(e), true) }
}

async function stopExp() {
  try { await invoke('experiment_stop', { deviceId: selectedDevice.value }); await loadStatus() }
  catch (e) { setMsg(String(e), true) }
}

async function startRecording() {
  try { await invoke('experiment_record_start', { deviceId: selectedDevice.value }); recording.value = true }
  catch (e) { setMsg(String(e), true) }
}

async function stopRecording() {
  try { await invoke('experiment_record_stop', { deviceId: selectedDevice.value }); recording.value = false }
  catch (e) { setMsg(String(e), true) }
}

async function setPageMode() {
  try {
    await invoke('experiment_pagemode', { deviceId: selectedDevice.value, mode: targetPageMode.value })
    setMsg('Page mode set to ' + targetPageMode.value)
  } catch (e) { setMsg(String(e), true) }
}

async function runChecksum() {
  try {
    checksumResult.value = await invoke('experiment_checksum', { deviceId: selectedDevice.value })
  } catch (e) { setMsg(String(e), true) }
}

async function loadOriginal(name, idx) {
  try {
    const r = await invoke('experiment_cal_original', { deviceId: selectedDevice.value, idx })
    originals[name] = r.value ?? '?'
    // Check if changed
    const ch = await invoke('experiment_cal_changed', { deviceId: selectedDevice.value, idx })
    changedMap[name] = ch.changed ?? false
  } catch (e) { setMsg(String(e), true) }
}

function writeVar(v) {
  writeTarget.value = v
  writeValue.value = ''
  writeMsg.value = ''
}

async function confirmWrite() {
  if (!writeTarget.value || !writeValue.value) return
  writeErr.value = false; writeMsg.value = ''
  try {
    const idx = vars.value.findIndex(v => v.name === writeTarget.value.name)
    await invoke('experiment_cal_write', { deviceId: selectedDevice.value, idx, value: writeValue.value })
    writeMsg.value = 'Written'
    // Update changed status
    const ch = await invoke('experiment_cal_changed', { deviceId: selectedDevice.value, idx })
    changedMap[writeTarget.value.name] = ch.changed ?? false
  } catch (e) { writeMsg.value = String(e); writeErr.value = true }
}

onMounted(async () => { await loadDevices(); await loadStatus() })
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 12px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 1rem; margin-bottom: 8px; }

.toolbar, .actions-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 10px; }
.sel { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; }
.sel-sm { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 4px 6px; font-family: inherit; font-size: 0.85rem; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; width: 100%; box-sizing: border-box; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: inherit; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
button.sm { padding: 2px 7px; font-size: 0.8rem; }
button.warn { border-color: #ffcc00; color: #ffcc00; }

.status-badge { font-size: 0.8rem; color: #444; padding: 3px 8px; border: 1px solid #222; }
.status-badge.active { color: #00ff66; border-color: #003311; }

.checksum-result { margin: 8px 0; padding: 6px 10px; font-weight: bold; }
.checksum-result.ok { color: #00ff66; }
.checksum-result.fail { color: #ff4444; }

.var-section { margin-top: 12px; }
.var-list { max-height: 360px; overflow-y: auto; border: 1px solid #001a09; margin-top: 8px; }
.var-row { padding: 7px 10px; border-bottom: 1px solid #001a09; display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
.var-main { display: flex; gap: 10px; align-items: baseline; flex: 1; min-width: 140px; }
.var-name { color: #00ff66; font-weight: bold; }
.var-type { color: #006622; font-size: 0.8rem; }
.var-vals { display: flex; gap: 10px; font-size: 0.8rem; }
.orig-val { color: #555; }
.curr-val { color: #00cc44; }
.changed-badge { background: #1a0a00; color: #ffcc00; padding: 1px 5px; font-size: 0.75rem; }
.var-actions { display: flex; gap: 4px; }

.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 100; }
.dialog { background: #0d0d0d; border: 1px solid #00cc44; padding: 20px; min-width: 320px; }
.dialog h3 { color: #00ff66; margin-bottom: 10px; }
.dialog .inp { margin-bottom: 10px; }
.row { display: flex; gap: 8px; }

.msg { margin-top: 8px; font-size: 0.85rem; color: #00cc44; }
.msg.err { color: #ff4444; }
</style>
