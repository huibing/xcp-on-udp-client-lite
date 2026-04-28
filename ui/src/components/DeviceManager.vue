<template>
  <div class="page">
    <div class="page-header">
      <h1>Slave Management</h1>
      <button type="button" @click="showAddModal = true">+ Add Slave</button>
      <button type="button" class="secondary" @click="connectAll" :disabled="isBusy('connect-all')">
        {{ isBusy('connect-all') ? 'Connecting...' : 'Connect All' }}
      </button>
      <button type="button" class="secondary" @click="refresh" :disabled="loadingPage">
        ↻ Refresh
      </button>
    </div>

    <div v-if="toast.message" class="toast" :class="toast.kind">{{ toast.message }}</div>

    <div v-if="loadingPage && devices.length === 0" class="card empty-state">
      Loading...
    </div>

    <div v-else-if="devices.length === 0" class="card empty-state">
      No slaves configured. Click <strong>+ Add Slave</strong> to get started.
    </div>

    <div v-else class="device-list">
      <div
        v-for="device in devices"
        :key="device.id"
        class="card device-card"
        :class="{ connected: device.connected }"
      >
        <div class="device-header">
          <input
            :value="renameDrafts[device.id] ?? device.name"
            class="name-input"
            title="Click to rename"
            @input="renameDrafts[device.id] = $event.target.value"
            @blur="renameSlave(device)"
            @keydown.enter="$event.target.blur()"
          />
          <span class="tag" :class="device.connected ? 'connected' : 'disconnected'">
            {{ device.connected ? 'Connected' : 'Disconnected' }}
          </span>
          <span class="device-address">{{ device.address || 'Address pending A2L load' }}</span>
        </div>

        <div class="device-grid">
          <div class="panel">
            <div class="panel-title">A2L File <span class="required">*</span></div>
            <div class="panel-value" :class="{ ok: device.a2l_loaded }">
              {{ device.a2l_loaded ? `✓ ${device.a2l_name || 'loaded'}` : 'Not loaded' }}
            </div>
            <button
              type="button"
              class="secondary"
              :disabled="isBusy(`${device.id}:a2l`)"
              @click="pickA2L(device)"
            >
              {{ isBusy(`${device.id}:a2l`) ? 'Loading...' : (device.a2l_loaded ? '↻ Replace A2L' : 'Load A2L') }}
            </button>
            <div v-if="device.a2l_loaded" class="panel-meta">
              {{ device.cal_count }} cal / {{ device.meas_count }} meas
            </div>
          </div>

          <div class="panel">
            <div class="panel-title">HEX File (optional)</div>
            <div class="panel-value" :class="{ ok: device.hex_loaded }">
              {{ device.hex_loaded ? `✓ ${device.hex_name || 'loaded'}` : 'Not loaded' }}
            </div>
            <button
              type="button"
              class="secondary"
              :disabled="isBusy(`${device.id}:hex`)"
              @click="pickHex(device)"
            >
              {{ isBusy(`${device.id}:hex`) ? 'Loading...' : (device.hex_loaded ? '↻ Replace HEX' : 'Load HEX') }}
            </button>
          </div>

          <div class="panel">
            <div class="panel-title">Checksum</div>
            <div class="panel-value">
              <span class="tag" :class="checksumClass(device.checksum_status)">
                {{ checksumText(device.checksum_status) }}
              </span>
            </div>
            <button
              type="button"
              class="secondary"
              :disabled="!device.connected || !device.hex_loaded || isBusy(`${device.id}:verify`)"
              @click="verifyChecksum(device)"
            >
              {{ isBusy(`${device.id}:verify`) ? 'Verifying...' : 'Run Verify' }}
            </button>
          </div>
        </div>

        <div class="device-actions">
          <select
            v-if="!device.connected"
            v-model="transportSelections[device.id]"
            class="sel"
          >
            <option
              v-for="option in device.transport_options"
              :key="`${device.id}-${option.value || 'auto'}`"
              :value="option.value"
            >
              {{ option.label }}
            </option>
          </select>

          <button
            v-if="device.connected"
            type="button"
            class="secondary"
            :disabled="isBusy(`${device.id}:connect`)"
            @click="disconnectDevice(device)"
          >
            {{ isBusy(`${device.id}:connect`) ? 'Working...' : 'Disconnect' }}
          </button>
          <button
            v-else
            type="button"
            :disabled="!device.a2l_loaded || isBusy(`${device.id}:connect`)"
            :title="device.a2l_loaded ? '' : 'Load A2L first'"
            @click="connectDevice(device)"
          >
            {{ isBusy(`${device.id}:connect`) ? 'Connecting...' : 'Connect' }}
          </button>

          <button
            type="button"
            class="danger"
            :disabled="isBusy(`${device.id}:remove`)"
            @click="removeDevice(device)"
          >
            {{ isBusy(`${device.id}:remove`) ? 'Removing...' : 'Remove' }}
          </button>
        </div>
      </div>
    </div>

    <div v-if="showAddModal" class="modal-overlay" @click.self="closeAddModal">
      <div class="modal">
        <h2>Add Slave</h2>
        <input
          v-model.trim="newSlaveName"
          class="inp"
          placeholder="Enter slave name (e.g. ECU1)"
          @keydown.enter="submitAddSlave"
        />
        <div class="modal-actions">
          <button type="button" @click="submitAddSlave" :disabled="!newSlaveName || isBusy('add')">
            {{ isBusy('add') ? 'Adding...' : 'Add' }}
          </button>
          <button type="button" class="secondary" @click="closeAddModal">Cancel</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { reactive, ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

const devices = ref([])
const loadingPage = ref(false)
const busyKey = ref('')
const showAddModal = ref(false)
const newSlaveName = ref('')
const transportSelections = reactive({})
const renameDrafts = reactive({})
const toast = reactive({ message: '', kind: 'info' })
let toastTimer = null

function isBusy(key) {
  return busyKey.value === key
}

function setBusy(key) {
  busyKey.value = key
}

function clearBusy(key) {
  if (busyKey.value === key) busyKey.value = ''
}

function showToast(message, kind = 'info') {
  toast.message = message
  toast.kind = kind
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    toast.message = ''
  }, 4500)
}

function checksumClass(status) {
  if (status === 'passed') return 'connected'
  if (status === 'failed') return 'disconnected'
  return 'stopped'
}

function checksumText(status) {
  if (status === 'passed') return '✓ Passed'
  if (status === 'failed') return '✗ Failed'
  return 'Not run'
}

function singlePath(result) {
  if (!result) return null
  return Array.isArray(result) ? (result[0] ?? null) : result
}

function syncDeviceDrafts() {
  const nextSelections = {}
  const nextNames = {}

  for (const device of devices.value) {
    const options = device.transport_options ?? []
    const currentSelection = transportSelections[device.id]
    const hasCurrent = options.some(option => option.value === currentSelection)
    nextSelections[device.id] = hasCurrent ? currentSelection : (options[0]?.value ?? '')
    nextNames[device.id] = renameDrafts[device.id] ?? device.name
  }

  for (const key of Object.keys(transportSelections)) delete transportSelections[key]
  for (const [key, value] of Object.entries(nextSelections)) transportSelections[key] = value

  for (const key of Object.keys(renameDrafts)) delete renameDrafts[key]
  for (const [key, value] of Object.entries(nextNames)) renameDrafts[key] = value
}

async function refresh() {
  loadingPage.value = true
  try {
    const result = await invoke('list_devices')
    devices.value = result.devices ?? []
    syncDeviceDrafts()
  } catch (error) {
    showToast(String(error), 'error')
  } finally {
    loadingPage.value = false
  }
}

function closeAddModal() {
  showAddModal.value = false
  newSlaveName.value = ''
}

async function submitAddSlave() {
  const name = newSlaveName.value.trim()
  if (!name) return

  setBusy('add')
  try {
    const result = await invoke('add_device', { name, address: null })
    if (!result.success) {
      showToast(result.error || 'Failed to add slave', 'error')
      return
    }
    closeAddModal()
    await refresh()
    showToast(result.message || `Added slave '${name}'`, 'success')
  } catch (error) {
    showToast(String(error), 'error')
  } finally {
    clearBusy('add')
  }
}

async function renameSlave(device) {
  const nextName = (renameDrafts[device.id] ?? '').trim()
  if (!nextName) {
    renameDrafts[device.id] = device.name
    return
  }
  if (nextName === device.name) return

  setBusy(`${device.id}:rename`)
  try {
    const result = await invoke('rename_device', {
      deviceId: device.id,
      name: nextName
    })
    if (!result.success) {
      showToast(result.error || 'Rename failed', 'error')
      renameDrafts[device.id] = device.name
      return
    }
    await refresh()
    showToast(result.message || `Renamed to '${nextName}'`, 'success')
  } catch (error) {
    renameDrafts[device.id] = device.name
    showToast(String(error), 'error')
  } finally {
    clearBusy(`${device.id}:rename`)
  }
}

async function connectDevice(device) {
  const key = `${device.id}:connect`
  setBusy(key)
  try {
    const result = await invoke('connect_device', {
      deviceId: device.id,
      transportProtocol: transportSelections[device.id] || ''
    })
    if (!result.success) {
      showToast(result.error || 'Connect failed', 'error')
      return
    }
    await refresh()
    showToast(result.message || `Connected to ${device.name}`, 'success')
  } catch (error) {
    showToast(String(error), 'error')
  } finally {
    clearBusy(key)
  }
}

async function disconnectDevice(device) {
  const key = `${device.id}:connect`
  setBusy(key)
  try {
    const result = await invoke('disconnect_device', { deviceId: device.id })
    if (!result.success) {
      showToast(result.error || 'Disconnect failed', 'error')
      return
    }
    await refresh()
    showToast(result.message || `Disconnected from ${device.name}`, 'success')
  } catch (error) {
    showToast(String(error), 'error')
  } finally {
    clearBusy(key)
  }
}

async function connectAll() {
  const protocolMap = {}
  for (const device of devices.value) {
    const value = transportSelections[device.id]
    if (value) protocolMap[device.id] = value
  }

  setBusy('connect-all')
  try {
    const result = await invoke('connect_all_devices', { transportProtocols: protocolMap })
    await refresh()
    showToast(result.message || 'Connect all finished', result.success ? 'success' : 'error')
  } catch (error) {
    showToast(String(error), 'error')
  } finally {
    clearBusy('connect-all')
  }
}

async function removeDevice(device) {
  if (!window.confirm('Remove this slave entry? This will also disconnect it if connected.')) {
    return
  }

  const key = `${device.id}:remove`
  setBusy(key)
  try {
    const result = await invoke('remove_device', { deviceId: device.id })
    if (!result.success) {
      showToast(result.error || 'Remove failed', 'error')
      return
    }
    await refresh()
    showToast(result.message || 'Device removed', 'success')
  } catch (error) {
    showToast(String(error), 'error')
  } finally {
    clearBusy(key)
  }
}

async function pickA2L(device) {
  const key = `${device.id}:a2l`
  setBusy(key)
  try {
    const filePath = singlePath(await open({
      multiple: false,
      directory: false,
      title: 'Load A2L File',
      filters: [{ name: 'A2L Files', extensions: ['a2l', 'A2L'] }]
    }))
    if (!filePath) return

    const result = await invoke('load_a2l', {
      deviceId: device.id,
      filePath
    })
    if (!result.success) {
      showToast(result.error || 'A2L load failed', 'error')
      return
    }
    await refresh()
    showToast(result.message || 'A2L loaded', 'success')
  } catch (error) {
    showToast(String(error), 'error')
  } finally {
    clearBusy(key)
  }
}

async function pickHex(device) {
  const key = `${device.id}:hex`
  setBusy(key)
  try {
    const filePath = singlePath(await open({
      multiple: false,
      directory: false,
      title: 'Load HEX File',
      filters: [{ name: 'HEX Files', extensions: ['hex', 'ihex', 'h86', 'HEX'] }]
    }))
    if (!filePath) return

    const result = await invoke('hex_load', {
      deviceId: device.id,
      filePath
    })
    if (!result.success) {
      showToast(result.error || 'HEX load failed', 'error')
      return
    }
    await refresh()
    showToast(result.message || 'HEX loaded', 'success')
  } catch (error) {
    showToast(String(error), 'error')
  } finally {
    clearBusy(key)
  }
}

async function verifyChecksum(device) {
  const key = `${device.id}:verify`
  setBusy(key)
  try {
    const result = await invoke('checksum_verify', {
      deviceId: device.id,
      maxBlockSize: null
    })
    if (!result.success) {
      showToast(result.error || 'Verification failed', 'error')
      return
    }
    await refresh()
    showToast(result.summary || 'Verification finished', result.all_passed ? 'success' : 'error')
  } catch (error) {
    showToast(String(error), 'error')
  } finally {
    clearBusy(key)
  }
}

onMounted(refresh)
</script>

<style scoped>
.page {
  color: #00cc44;
}

.page-header {
  display: flex;
  gap: 0.75rem;
  align-items: center;
  flex-wrap: wrap;
  margin-bottom: 1.5rem;
}

h1 {
  color: #00ff66;
  margin: 0;
  font-size: 1.4rem;
}

h2 {
  margin: 0 0 0.9rem;
  color: #00ff66;
}

.card {
  background: #0d0d0d;
  border: 1px solid #0f3460;
  padding: 1rem;
  border-radius: 8px;
}

.device-list {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.device-card.connected {
  border-color: #00ff66;
}

.device-header {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-wrap: wrap;
  margin-bottom: 1rem;
}

.device-address {
  margin-left: auto;
  color: #888;
  font-size: 0.85rem;
}

.device-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 0.75rem;
  margin-bottom: 1rem;
}

.panel {
  background: #111827;
  border: 1px solid #003311;
  border-radius: 6px;
  padding: 0.85rem;
}

.panel-title {
  color: #888;
  font-size: 0.8rem;
  margin-bottom: 0.35rem;
}

.panel-value {
  min-height: 1.5rem;
  margin-bottom: 0.6rem;
  color: #888;
}

.panel-value.ok {
  color: #00ff66;
}

.panel-meta {
  margin-top: 0.4rem;
  color: #888;
  font-size: 0.8rem;
}

.required {
  color: #ff4444;
}

.device-actions {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
}

.sel,
.inp,
.name-input {
  background: #0a0a0a;
  border: 1px solid #003311;
  color: #00cc44;
  padding: 0.45rem 0.65rem;
  font-family: Consolas, monospace;
  font-size: 0.9rem;
}

.sel {
  min-width: 220px;
}

.name-input {
  width: 220px;
  font-size: 1.05rem;
  font-weight: 600;
  border-color: transparent;
}

.name-input:focus {
  border-color: #00cc44;
  outline: none;
}

button {
  background: #001a09;
  border: 1px solid #00cc44;
  color: #00cc44;
  padding: 0.45rem 0.9rem;
  cursor: pointer;
  font-family: Consolas, monospace;
}

button:hover:not(:disabled) {
  background: #003311;
}

button:disabled {
  opacity: 0.35;
  cursor: default;
}

button.secondary {
  border-color: #0f3460;
  color: #9cc8ff;
}

button.secondary:hover:not(:disabled) {
  background: #10213b;
}

button.danger {
  border-color: #ff4444;
  color: #ff4444;
  margin-left: auto;
}

button.danger:hover:not(:disabled) {
  background: #2a0b0b;
}

.tag {
  display: inline-flex;
  align-items: center;
  border: 1px solid #333;
  padding: 0.2rem 0.55rem;
  font-size: 0.78rem;
  border-radius: 999px;
}

.tag.connected {
  color: #00ff66;
  border-color: #006622;
}

.tag.disconnected {
  color: #ff6666;
  border-color: #5c1f1f;
}

.tag.stopped {
  color: #c2a941;
  border-color: #5b4b18;
}

.empty-state {
  color: #888;
  text-align: center;
  padding: 2rem;
}

.modal-overlay {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.7);
}

.modal {
  background: #0d0d0d;
  border: 1px solid #003311;
  border-radius: 8px;
  padding: 1.25rem;
  min-width: 360px;
}

.modal-actions {
  display: flex;
  gap: 0.5rem;
  margin-top: 1rem;
}

.toast {
  position: fixed;
  top: 1rem;
  right: 1rem;
  z-index: 200;
  padding: 0.8rem 1rem;
  border-radius: 6px;
  background: #111;
  border: 1px solid #333;
  color: #e5e7eb;
  max-width: 420px;
}

.toast.success {
  border-color: #006622;
  color: #00ff66;
}

.toast.error {
  border-color: #5c1f1f;
  color: #ff6666;
}

@media (max-width: 720px) {
  .device-address {
    width: 100%;
    margin-left: 0;
  }

  .name-input,
  .sel {
    width: 100%;
  }

  button.danger {
    margin-left: 0;
  }
}
</style>
