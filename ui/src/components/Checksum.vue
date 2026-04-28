<template>
  <div class="page">
    <h1>HEX Checksum Verification</h1>

    <div class="card">
      <h2>Device &amp; HEX File</h2>
      <div class="row">
        <select v-model="selectedDevice" class="sel">
          <option value="">Select device…</option>
          <option v-for="d in connectedDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
        </select>
      </div>
      <div class="row" style="margin-top:10px">
        <span class="path-display">{{ hexPath || '(no file selected)' }}</span>
        <button @click="browseHex">Browse…</button>
        <button @click="loadHex" :disabled="!selectedDevice || !hexPath || loading">
          {{ loading ? 'Loading…' : 'Load HEX' }}
        </button>
        <label class="check-label">
          <input type="checkbox" v-model="autoVerify" /> Auto-verify after load
        </label>
      </div>
      <div v-if="hexLoaded" class="hex-info">✓ HEX loaded: {{ sectionCount }} section(s) — {{ hexFilename }}</div>
    </div>

    <div class="card">
      <h2>Verify Checksums</h2>
      <button @click="verify" :disabled="!hexLoaded || !selectedDevice || verifying">
        {{ verifying ? 'Verifying…' : '⚙ Verify Against ECU' }}
      </button>

      <template v-if="results.length > 0">
        <div class="summary-row">
          <div class="sum-card">
            <div class="sum-label">Total</div>
            <div class="sum-val">{{ results.length }}</div>
          </div>
          <div class="sum-card ok">
            <div class="sum-label">Passed</div>
            <div class="sum-val green">{{ passCount }}</div>
          </div>
          <div class="sum-card" :class="failCount > 0 ? 'fail' : ''">
            <div class="sum-label">Failed</div>
            <div class="sum-val" :class="failCount > 0 ? 'red' : ''">{{ failCount }}</div>
          </div>
        </div>

        <div class="banner" :class="failCount === 0 ? 'pass' : 'fail'">
          {{ failCount === 0 ? '✓ ALL SEGMENTS PASS' : '✗ CHECKSUM MISMATCH DETECTED' }}
        </div>

        <table class="seg-table">
          <thead>
            <tr><th>Segment</th><th>Address</th><th>Size</th><th>Expected</th><th>Actual</th><th>Match</th></tr>
          </thead>
          <tbody>
            <tr v-for="s in results" :key="s.name + s.address" :class="s.match ? 'ok-row' : 'fail-row'">
              <td>{{ s.name || '—' }}</td>
              <td class="mono">{{ s.address }}</td>
              <td class="mono">{{ s.size ?? '—' }}</td>
              <td class="mono">{{ s.expected }}</td>
              <td class="mono">{{ s.actual }}</td>
              <td class="match-cell">{{ s.match ? '✓' : '✗' }}</td>
            </tr>
          </tbody>
        </table>
      </template>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

const connectedDevices = ref([])
const selectedDevice = ref('')
const hexPath = ref('')
const hexFilename = ref('')
const hexLoaded = ref(false)
const sectionCount = ref(0)
const autoVerify = ref(false)
const results = ref([])
const loading = ref(false)
const verifying = ref(false)
const msg = ref('')
const msgErr = ref(false)

const passCount = computed(() => results.value.filter(s => s.match).length)
const failCount = computed(() => results.value.filter(s => !s.match).length)

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 5000)
}

async function loadDevices() {
  try {
    const r = await invoke('list_devices')
    connectedDevices.value = (r.devices ?? []).filter(d => d.connected)
    if (connectedDevices.value.length === 1) selectedDevice.value = connectedDevices.value[0].id
  } catch (e) { setMsg(String(e), true) }
}

async function browseHex() {
  try {
    const path = await open({
      multiple: false,
      directory: false,
      title: 'Select HEX File',
      filters: [{ name: 'HEX Files', extensions: ['hex', 'HEX'] }]
    })
    if (path) {
      hexPath.value = path
      hexFilename.value = path.split('\\').pop() || path.split('/').pop() || path
      hexLoaded.value = false
      results.value = []
    }
  } catch (e) { setMsg(String(e), true) }
}

async function loadHex() {
  if (!selectedDevice.value || !hexPath.value) return
  loading.value = true
  hexLoaded.value = false
  results.value = []
  try {
    const r = await invoke('hex_load', { deviceId: selectedDevice.value, filePath: hexPath.value })
    if (r.success) {
      hexLoaded.value = true
      sectionCount.value = r.byte_count ?? 0
      hexFilename.value = r.filename ?? hexFilename.value
      setMsg(r.message || `HEX loaded: ${sectionCount.value} bytes`)
      if (autoVerify.value) await verify()
    } else {
      setMsg(r.error || 'Failed to load HEX', true)
    }
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = false }
}

async function verify() {
  if (!selectedDevice.value) return
  verifying.value = true
  results.value = []
  try {
    const r = await invoke('checksum_verify', { deviceId: selectedDevice.value })
    results.value = (r.segments ?? []).map(s => ({
      name: s.name,
      address: s.address,
      size: s.size,
      expected: s.master_checksum,
      actual: s.slave_checksum,
      match: s.match
    }))
  } catch (e) { setMsg(String(e), true) }
  finally { verifying.value = false }
}

onMounted(loadDevices)
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 16px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 1rem; margin-bottom: 10px; }
.card { background: #0d0d0d; border: 1px solid #003311; padding: 14px; margin-bottom: 16px; }
.row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.path-display { color: #006622; font-family: Consolas, monospace; font-size: 0.85rem; flex: 1; min-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.check-label { display: flex; align-items: center; gap: 4px; color: #006622; font-size: 0.85rem; cursor: pointer; }
.check-label input { accent-color: #00cc44; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: Consolas, monospace; font-size: 0.9rem; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
.sel { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: Consolas, monospace; }
.hex-info { margin-top: 10px; color: #00ff66; font-size: 0.85rem; }
.summary-row { display: flex; gap: 12px; margin: 14px 0 8px; }
.sum-card { background: #111; border: 1px solid #003311; padding: 8px 14px; min-width: 80px; }
.sum-card.ok { border-color: #006622; }
.sum-card.fail { border-color: #660000; }
.sum-label { color: #555; font-size: 0.72rem; text-transform: uppercase; margin-bottom: 2px; }
.sum-val { font-size: 1.4rem; color: #00cc44; }
.sum-val.green { color: #00ff66; }
.sum-val.red { color: #ff4444; }
.banner { padding: 8px 14px; font-weight: bold; margin-bottom: 12px; border: 1px solid; }
.banner.pass { color: #00ff66; border-color: #006622; background: #001a09; }
.banner.fail { color: #ff4444; border-color: #660000; background: #1a0000; }
.seg-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
.seg-table th { color: #006622; text-align: left; padding: 5px 8px; border-bottom: 1px solid #003311; }
.seg-table td { padding: 5px 8px; border-bottom: 1px solid #001a09; }
.ok-row td { color: #00cc44; }
.fail-row td { color: #ff4444; }
.mono { font-family: Consolas, monospace; }
.match-cell { font-weight: bold; text-align: center; }
.msg { margin-top: 8px; font-size: 0.85rem; color: #00cc44; }
.msg.err { color: #ff4444; }
</style>
