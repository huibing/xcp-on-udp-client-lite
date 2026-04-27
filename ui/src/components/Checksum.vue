<template>
  <div class="page">
    <h1>HEX Checksum Verification</h1>

    <div class="card">
      <h2>Load HEX File</h2>
      <div class="row">
        <select v-model="selectedDevice" class="sel">
          <option value="">Select device…</option>
          <option v-for="d in connectedDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
        </select>
        <input v-model="hexPath" placeholder="Path to .hex file" class="inp wide" @keyup.enter="loadHex" />
        <button @click="loadHex" :disabled="!selectedDevice || !hexPath || loading">Load HEX</button>
      </div>
      <div v-if="hexLoaded" class="hex-info">✓ HEX loaded: {{ hexFilename }}</div>
    </div>

    <div class="card">
      <h2>Verify Checksums</h2>
      <button @click="verify" :disabled="!hexLoaded || verifying">
        {{ verifying ? 'Verifying…' : 'Verify Against ECU' }}
      </button>
      <div v-if="report" class="report">
        <div class="report-header" :class="report.all_match ? 'ok' : 'fail'">
          {{ report.all_match ? '✓ ALL SEGMENTS MATCH' : '✗ CHECKSUM MISMATCH' }}
        </div>
        <table class="seg-table">
          <thead><tr><th>Segment</th><th>Address</th><th>HEX CRC</th><th>ECU CRC</th><th>Match</th></tr></thead>
          <tbody>
            <tr v-for="s in report.segments" :key="s.name || s.address" :class="s.match ? 'ok-row' : 'fail-row'">
              <td>{{ s.name || '—' }}</td>
              <td>{{ s.address }}</td>
              <td>{{ s.hex_crc }}</td>
              <td>{{ s.ecu_crc }}</td>
              <td>{{ s.match ? '✓' : '✗' }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const connectedDevices = ref([])
const selectedDevice = ref('')
const hexPath = ref('')
const hexLoaded = ref(false)
const hexFilename = ref('')
const report = ref(null)
const loading = ref(false)
const verifying = ref(false)
const msg = ref('')
const msgErr = ref(false)

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 5000)
}

async function loadDevices() {
  const r = await invoke('list_devices')
  connectedDevices.value = r.devices.filter(d => d.connected)
  if (connectedDevices.value.length === 1) selectedDevice.value = connectedDevices.value[0].id
}

async function loadHex() {
  if (!selectedDevice.value || !hexPath.value) return
  loading.value = true
  try {
    await invoke('hex_load', { deviceId: selectedDevice.value, path: hexPath.value })
    hexLoaded.value = true
    hexFilename.value = hexPath.value.split('\\').pop() || hexPath.value
    setMsg('HEX file loaded successfully')
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = false }
}

async function verify() {
  verifying.value = true; report.value = null
  try {
    const r = await invoke('checksum_verify', { deviceId: selectedDevice.value })
    report.value = r
  } catch (e) { setMsg(String(e), true) }
  finally { verifying.value = false }
}

onMounted(loadDevices)
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 16px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 1rem; margin-bottom: 10px; }

.card { background: #0d0d0d; border: 1px solid #003311; padding: 14px; border-radius: 4px; margin-bottom: 16px; }
.row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; font-size: 0.9rem; }
.inp.wide { flex: 1; min-width: 200px; }
.sel { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 12px; cursor: pointer; font-family: inherit; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }

.hex-info { margin-top: 8px; color: #00ff66; font-size: 0.85rem; }

.report { margin-top: 12px; }
.report-header { padding: 8px 12px; font-weight: bold; border-radius: 2px; margin-bottom: 8px; }
.report-header.ok { background: #001a09; color: #00ff66; border: 1px solid #00cc44; }
.report-header.fail { background: #1a0000; color: #ff4444; border: 1px solid #ff4444; }

.seg-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
.seg-table th { color: #006622; text-align: left; padding: 4px 8px; border-bottom: 1px solid #003311; }
.seg-table td { padding: 5px 8px; border-bottom: 1px solid #001a09; }
.ok-row td { color: #00cc44; }
.fail-row td { color: #ff4444; }

.msg { margin-top: 8px; font-size: 0.85rem; color: #00cc44; }
.msg.err { color: #ff4444; }
</style>
