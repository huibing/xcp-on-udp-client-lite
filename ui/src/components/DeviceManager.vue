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

          <div class="panel" v-if="device.hex_loaded">
            <div class="panel-title">Working Page HEX</div>
            <div class="panel-value" :class="{ ok: !!device.working_hex_name }">
              {{ device.working_hex_name || '(auto-derived from reference)' }}
            </div>
            <button
              type="button"
              class="secondary"
              :disabled="isBusy(`${device.id}:working_hex`)"
              @click="pickWorkingHex(device)"
              title="Upload a dedicated HEX file as the working page instead of the auto-derived copy"
            >
              {{ isBusy(`${device.id}:working_hex`) ? 'Loading...' : '↻ Set Working HEX' }}
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

          <div class="panel">
            <div class="panel-title">ECU Upload</div>
            <div class="panel-value">Save ECU memory to HEX file(s)</div>
            <button
              type="button"
              class="secondary"
              :disabled="!device.connected || !device.a2l_loaded || isBusy(`${device.id}:upload`)"
              :title="!device.connected ? 'Connect first' : !device.a2l_loaded ? 'Load A2L first' : 'Upload ECU memory to HEX file'"
              @click="openUploadModal(device)"
            >
              {{ isBusy(`${device.id}:upload`) ? 'Uploading...' : 'Upload ECU...' }}
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

          <select
            v-if="!device.connected && canSelectorVisible(device)"
            v-model="canSelections[device.id]"
            class="sel"
            title="CAN device / channel"
          >
            <option value="">Auto (first available CAN channel)</option>
            <option
              v-for="canDevice in canDevices"
              :key="`${device.id}-${canDevice.value}`"
              :value="canDevice.value"
            >
              {{ canOptionLabel(canDevice) }}
            </option>
            <option v-if="!canDevices.length" value="" disabled>
              No CAN devices found
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

    <!-- ECU Upload Modal -->
    <div v-if="uploadModal.open" class="modal-overlay" @click.self="closeUploadModal">
      <div class="modal upload-modal">
        <h2>Upload ECU Memory</h2>
        <p class="modal-device-name">Device: <strong>{{ uploadModal.deviceName }}</strong></p>

        <div class="modal-section">
          <div class="modal-label">Calibration Page</div>
          <div class="modal-radio-group">
            <label><input type="radio" v-model="uploadModal.pages" value="both" /> Both (Reference + Working)</label>
            <label><input type="radio" v-model="uploadModal.pages" value="reference" /> Reference only</label>
            <label><input type="radio" v-model="uploadModal.pages" value="working" /> Working only</label>
          </div>
        </div>

        <div class="modal-section">
          <div class="modal-label">
            Memory Segments
            <span class="modal-hint">({{ uploadModal.selectedIndices.length }}/{{ uploadModal.segments.length }} selected)</span>
          </div>
          <div v-if="uploadModal.loadingSegments" class="modal-loading">Loading segments…</div>
          <div v-else-if="uploadModal.segments.length === 0" class="modal-empty">No memory segments found in A2L</div>
          <div v-else class="segment-list">
            <label
              v-for="seg in uploadModal.segments"
              :key="seg.index"
              class="segment-row"
            >
              <input type="checkbox" :value="seg.index" v-model="uploadModal.selectedIndices" />
              <span class="seg-name">{{ seg.name }}</span>
              <span class="seg-addr">{{ seg.address }}</span>
              <span class="seg-size">{{ seg.size_kb > 0 ? seg.size_kb + ' KB' : seg.size + ' B' }}</span>
            </label>
            <div class="segment-select-all">
              <button type="button" class="secondary small" @click="selectAllSegments">All</button>
              <button type="button" class="secondary small" @click="uploadModal.selectedIndices = []">None</button>
            </div>
          </div>
        </div>

        <div class="modal-section">
          <div class="modal-label">Output Folder</div>
          <div class="folder-row">
            <span class="folder-path">{{ uploadModal.folder || '(not selected)' }}</span>
            <button type="button" class="secondary" @click="pickUploadFolder" :disabled="uploadModal.busy">Browse…</button>
          </div>
        </div>

        <!-- Progress bar (visible during upload) -->
        <div v-if="uploadModal.busy && uploadModal.progress" class="progress-section">
          <div class="progress-label">
            <span class="progress-page">{{ uploadModal.progress.page ? `[${uploadModal.progress.page}]` : '' }}</span>
            <span class="progress-msg">{{ uploadModal.progress.message }}</span>
            <span class="progress-pct">{{ Math.round(uploadModal.progress.fraction * 100) }}%</span>
          </div>
          <div class="progress-track">
            <div class="progress-fill" :style="{ width: (uploadModal.progress.fraction * 100) + '%' }"></div>
          </div>
          <div class="progress-detail">
            Segment {{ uploadModal.progress.segmentsDone }}/{{ uploadModal.progress.segmentsTotal }}
            <span v-if="uploadModal.progress.pagesTotal > 1">
              — Page {{ uploadModal.progress.pageIndex + 1 }}/{{ uploadModal.progress.pagesTotal }}
            </span>
          </div>
        </div>

        <div v-if="uploadModal.result" class="modal-result" :class="uploadModal.result.success ? 'ok' : 'err'">
          {{ uploadModal.result.success ? uploadModal.result.message : uploadModal.result.error }}
          <ul v-if="uploadModal.result.warnings && uploadModal.result.warnings.length" class="warnings-list">
            <li v-for="w in uploadModal.result.warnings" :key="w">⚠ {{ w }}</li>
          </ul>
        </div>

        <div class="modal-actions">
          <button
            type="button"
            :disabled="!uploadModal.folder || uploadModal.selectedIndices.length === 0 || uploadModal.busy"
            @click="startUpload"
          >
            {{ uploadModal.busy ? 'Uploading…' : 'Start Upload' }}
          </button>
          <button type="button" class="secondary" @click="closeUploadModal" :disabled="uploadModal.busy">Cancel</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { reactive, ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'

const devices = ref([])
const loadingPage = ref(false)
const busyKey = ref('')
const showAddModal = ref(false)
const newSlaveName = ref('')
const transportSelections = reactive({})
const canSelections = reactive({})
const canDevices = ref([])
const renameDrafts = reactive({})
const toast = reactive({ message: '', kind: 'info' })
let toastTimer = null

// ── Upload modal state ────────────────────────────────────────────────────────
const uploadModal = reactive({
  open: false,
  deviceId: null,
  deviceName: '',
  pages: 'both',
  segments: [],
  selectedIndices: [],
  loadingSegments: false,
  folder: '',
  busy: false,
  result: null,
  progress: null,   // latest UploadProgressEvent
})
let progressUnlisten = null

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
  const nextCanSelections = {}

  for (const device of devices.value) {
    const options = device.transport_options ?? []
    const currentSelection = transportSelections[device.id]
    const hasCurrent = options.some(option => option.value === currentSelection)
    nextSelections[device.id] = hasCurrent ? currentSelection : (options[0]?.value ?? '')
    nextNames[device.id] = renameDrafts[device.id] ?? device.name
    nextCanSelections[device.id] = canSelections[device.id] ?? ''
  }

  for (const key of Object.keys(transportSelections)) delete transportSelections[key]
  for (const [key, value] of Object.entries(nextSelections)) transportSelections[key] = value

  for (const key of Object.keys(renameDrafts)) delete renameDrafts[key]
  for (const [key, value] of Object.entries(nextNames)) renameDrafts[key] = value

  for (const key of Object.keys(canSelections)) delete canSelections[key]
  for (const [key, value] of Object.entries(nextCanSelections)) canSelections[key] = value
}

async function refreshCanDevices() {
  try {
    const result = await invoke('list_can_devices')
    canDevices.value = result.devices ?? []
  } catch (error) {
    canDevices.value = []
    console.warn('Failed to list CAN devices', error)
  }
}

function canSelectorVisible(device) {
  const options = device.transport_options ?? []
  const hasCan = options.some(option => option.value === 'can')
  return hasCan && transportSelections[device.id] === 'can'
}

function canOptionLabel(canDevice) {
  const fd = canDevice.fd_capable ? ' [FD]' : ''
  const state = canDevice.available ? '' : ' (in use)'
  const base = canDevice.label || `${canDevice.name} \u00b7 ${canDevice.channel}`
  return `${base}${fd}${state}`
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
      transportProtocol: transportSelections[device.id] || '',
      canDevice: canSelections[device.id] || ''
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
  const canMap = {}
  for (const device of devices.value) {
    const value = transportSelections[device.id]
    if (value) protocolMap[device.id] = value
    const canValue = canSelections[device.id]
    if (canValue) canMap[device.id] = canValue
  }

  setBusy('connect-all')
  try {
    const result = await invoke('connect_all_devices', {
      transportProtocols: protocolMap,
      canDevices: canMap
    })
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

async function pickWorkingHex(device) {
  const key = `${device.id}:working_hex`
  setBusy(key)
  try {
    const filePath = singlePath(await open({
      multiple: false,
      directory: false,
      title: 'Load Working Page HEX File',
      filters: [{ name: 'HEX Files', extensions: ['hex', 'ihex', 'h86', 'HEX'] }]
    }))
    if (!filePath) return

    const result = await invoke('working_hex_load', {
      deviceId: device.id,
      filePath
    })
    if (!result.success) {
      showToast(result.error || 'Working HEX load failed', 'error')
      return
    }
    await refresh()
    showToast(result.message || 'Working page HEX loaded', 'success')
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

// ── Upload functions ──────────────────────────────────────────────────────────

async function openUploadModal(device) {
  uploadModal.open = true
  uploadModal.deviceId = device.id
  uploadModal.deviceName = device.name
  uploadModal.pages = 'both'
  uploadModal.segments = []
  uploadModal.selectedIndices = []
  uploadModal.folder = ''
  uploadModal.busy = false
  uploadModal.result = null
  uploadModal.loadingSegments = true

  try {
    const result = await invoke('upload_get_segments', { deviceId: device.id })
    uploadModal.segments = result.segments ?? []
    // Pre-select all segments
    uploadModal.selectedIndices = uploadModal.segments.map(s => s.index)
  } catch (err) {
    showToast(String(err), 'error')
  } finally {
    uploadModal.loadingSegments = false
  }
}

function closeUploadModal() {
  if (uploadModal.busy) return
  uploadModal.open = false
  if (progressUnlisten) { progressUnlisten(); progressUnlisten = null }
}

function selectAllSegments() {
  uploadModal.selectedIndices = uploadModal.segments.map(s => s.index)
}

async function pickUploadFolder() {
  try {
    const folder = singlePath(await open({
      multiple: false,
      directory: true,
      title: 'Select Output Folder for HEX Files',
    }))
    if (folder) uploadModal.folder = folder
  } catch (err) {
    showToast(String(err), 'error')
  }
}

async function startUpload() {
  if (!uploadModal.folder || uploadModal.selectedIndices.length === 0) return
  uploadModal.busy = true
  uploadModal.result = null
  uploadModal.progress = null
  const key = `${uploadModal.deviceId}:upload`
  setBusy(key)

  // Subscribe to progress events (filter by device_id)
  if (progressUnlisten) { progressUnlisten(); progressUnlisten = null }
  progressUnlisten = await listen('upload-progress', (event) => {
    if (event.payload.deviceId === uploadModal.deviceId) {
      uploadModal.progress = event.payload
    }
  })

  try {
    const result = await invoke('upload_memory', {
      deviceId: uploadModal.deviceId,
      segmentIndices: uploadModal.selectedIndices,
      pages: uploadModal.pages,
      outputFolder: uploadModal.folder,
    })
    uploadModal.result = result
    if (result.success) {
      showToast(result.message || 'Upload complete', 'success')
    } else {
      showToast(result.error || 'Upload failed', 'error')
    }
  } catch (err) {
    uploadModal.result = { success: false, error: String(err) }
    showToast(String(err), 'error')
  } finally {
    uploadModal.busy = false
    clearBusy(key)
    if (progressUnlisten) { progressUnlisten(); progressUnlisten = null }
  }
}

onMounted(() => {
  refresh()
  refreshCanDevices()
})
</script>

<style scoped>
.page {
  color: var(--exp-fg, #00cc44);
}

.page-header {
  display: flex;
  gap: 0.75rem;
  align-items: center;
  flex-wrap: wrap;
  margin-bottom: 1.5rem;
}

h1 {
  color: var(--exp-fg-bright, #00ff66);
  margin: 0;
  font-size: 1.4rem;
}

h2 {
  margin: 0 0 0.9rem;
  color: var(--exp-fg-bright, #00ff66);
}

.card {
  background: var(--exp-bg3, #0d0d0d);
  border: 1px solid var(--exp-border, #0f3460);
  padding: 1rem;
  border-radius: 8px;
}

.device-list {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.device-card.connected {
  border-color: var(--exp-fg-bright, #00ff66);
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
  color: var(--exp-fg-dim, #888);
  font-size: 0.85rem;
}

.device-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 0.75rem;
  margin-bottom: 1rem;
}

.panel {
  background: var(--exp-bg3, #111827);
  border: 1px solid var(--exp-border, #003311);
  border-radius: 6px;
  padding: 0.85rem;
}

.panel-title {
  color: var(--exp-fg-dim, #888);
  font-size: 0.8rem;
  margin-bottom: 0.35rem;
}

.panel-value {
  min-height: 1.5rem;
  margin-bottom: 0.6rem;
  color: var(--exp-fg-dim, #888);
}

.panel-value.ok {
  color: var(--exp-fg-bright, #00ff66);
}

.panel-meta {
  margin-top: 0.4rem;
  color: var(--exp-fg-dim, #888);
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
  background: var(--exp-bg4, #0a0a0a);
  border: 1px solid var(--exp-border, #003311);
  color: var(--exp-fg, #00cc44);
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
  border-color: var(--exp-fg, #00cc44);
  outline: none;
}

button {
  background: var(--exp-border3, #001a09);
  border: 1px solid var(--exp-fg, #00cc44);
  color: var(--exp-fg, #00cc44);
  padding: 0.45rem 0.9rem;
  cursor: pointer;
  font-family: Consolas, monospace;
}

button:hover:not(:disabled) {
  background: var(--exp-hover, #003311);
}

button:disabled {
  opacity: 0.35;
  cursor: default;
}

button.secondary {
  border-color: var(--exp-border, #0f3460);
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
  color: var(--exp-fg-bright, #00ff66);
  border-color: var(--exp-fg-dim3, #006622);
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
  color: var(--exp-fg-dim, #888);
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
  background: var(--exp-bg3, #0d0d0d);
  border: 1px solid var(--exp-border, #003311);
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
  background: var(--exp-bg5, #111);
  border: 1px solid #333;
  color: #e5e7eb;
  max-width: 420px;
}

.toast.success {
  border-color: var(--exp-fg-dim3, #006622);
  color: var(--exp-fg-bright, #00ff66);
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

/* ── Upload modal ──────────────────────────────────────────────────────────── */

.upload-modal {
  min-width: 480px;
  max-width: 640px;
  max-height: 85vh;
  overflow-y: auto;
}

.modal-device-name {
  margin: 0 0 0.75rem;
  color: var(--exp-fg-dim, #888);
  font-size: 0.9rem;
}

.modal-section {
  margin-bottom: 1rem;
}

.modal-label {
  color: var(--exp-fg-dim, #888);
  font-size: 0.8rem;
  margin-bottom: 0.35rem;
}

.modal-hint {
  color: var(--exp-fg-dim, #555);
  font-size: 0.75rem;
  margin-left: 0.4rem;
}

.modal-radio-group {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.modal-radio-group label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
  font-size: 0.9rem;
  color: var(--exp-fg, #00cc44);
}

.modal-loading,
.modal-empty {
  color: var(--exp-fg-dim, #888);
  font-size: 0.85rem;
  padding: 0.4rem 0;
}

.segment-list {
  background: var(--exp-bg4, #0a0a0a);
  border: 1px solid var(--exp-border, #003311);
  border-radius: 4px;
  max-height: 220px;
  overflow-y: auto;
  padding: 0.4rem;
}

.segment-row {
  display: grid;
  grid-template-columns: auto 1fr auto auto;
  gap: 0.5rem;
  align-items: center;
  padding: 0.3rem 0.4rem;
  cursor: pointer;
  border-radius: 3px;
  font-size: 0.85rem;
}

.segment-row:hover {
  background: var(--exp-hover, #003311);
}

.seg-name {
  color: var(--exp-fg, #00cc44);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.seg-addr {
  color: var(--exp-fg-dim, #888);
  font-family: Consolas, monospace;
  font-size: 0.8rem;
}

.seg-size {
  color: var(--exp-fg-dim, #888);
  font-size: 0.8rem;
  text-align: right;
}

.segment-select-all {
  display: flex;
  gap: 0.4rem;
  padding: 0.35rem 0.4rem 0;
}

button.small {
  padding: 0.25rem 0.6rem;
  font-size: 0.8rem;
}

.folder-row {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  flex-wrap: wrap;
}

.folder-path {
  flex: 1;
  color: var(--exp-fg-dim, #888);
  font-size: 0.82rem;
  font-family: Consolas, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.modal-result {
  padding: 0.6rem 0.8rem;
  border-radius: 4px;
  margin-bottom: 0.75rem;
  font-size: 0.88rem;
}

.modal-result.ok {
  border: 1px solid var(--exp-fg-dim3, #006622);
  color: var(--exp-fg-bright, #00ff66);
}

.modal-result.err {
  border: 1px solid #5c1f1f;
  color: #ff6666;
}

.warnings-list {
  margin: 0.4rem 0 0;
  padding-left: 1.2rem;
  color: #c2a941;
  font-size: 0.82rem;
}

/* ── Progress bar ──────────────────────────────────────────────────────────── */

.progress-section {
  margin-bottom: 0.75rem;
}

.progress-label {
  display: flex;
  align-items: baseline;
  gap: 0.5rem;
  margin-bottom: 0.35rem;
  font-size: 0.82rem;
}

.progress-page {
  color: var(--exp-fg-bright, #00ff66);
  font-family: Consolas, monospace;
  white-space: nowrap;
}

.progress-msg {
  flex: 1;
  color: var(--exp-fg, #00cc44);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.progress-pct {
  color: var(--exp-fg-dim, #888);
  font-family: Consolas, monospace;
  white-space: nowrap;
}

.progress-track {
  height: 8px;
  background: var(--exp-bg4, #0a0a0a);
  border: 1px solid var(--exp-border, #003311);
  border-radius: 4px;
  overflow: hidden;
  margin-bottom: 0.3rem;
}

.progress-fill {
  height: 100%;
  background: var(--exp-fg-bright, #00ff66);
  border-radius: 4px;
  transition: width 0.3s ease;
}

.progress-detail {
  color: var(--exp-fg-dim, #888);
  font-size: 0.78rem;
}
</style>
