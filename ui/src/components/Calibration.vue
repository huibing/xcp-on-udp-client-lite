<template>
  <div class="page">
    <h1>Calibration</h1>

    <div class="toolbar">
      <input v-model="search" placeholder="Search parameters…" class="inp" />
      <select v-model="deviceFilter" class="sel">
        <option value="">All devices</option>
        <option v-for="d in connectedDevices" :key="d.id" :value="d.id">{{ d.name }}</option>
      </select>
      <button @click="loadAll">↻ Refresh</button>
      <button @click="readAll" :disabled="readingAll || displayedParams.length === 0">
        {{ readingAll ? `Reading ${readAllProgress}/${displayedParams.length}…` : '⬇ Read All' }}
      </button>
      <button @click="openAddModal">+ Add Variables</button>
    </div>

    <div v-if="connectedDevices.length > 0" class="page-mode-bar">
      <span class="pm-label">Page Mode:</span>
      <span v-for="d in connectedDevices" :key="d.id" class="pm-device">
        <span class="pm-name">{{ d.name }}</span>
        <button @click="togglePage(d.id)" :disabled="togglingPage === d.id" class="pm-btn"
          :class="pageModes[d.id] === 'Working' ? 'working' : 'reference'">
          {{ pageModes[d.id] || '…' }}
        </button>
      </span>
    </div>

    <div class="count">{{ displayedParams.length }} selected parameter(s)</div>

    <div v-if="displayedParams.length === 0" class="empty">
      No parameters selected. Click &quot;+ Add Variables&quot; to choose parameters.
    </div>

    <div v-else class="table-wrap">
      <table class="param-table">
        <thead>
          <tr><th>Name / Device</th><th>Type</th><th>Address</th><th>Value</th><th>Actions</th></tr>
        </thead>
        <tbody>
          <tr v-for="p in displayedParams" :key="p.device_id + ':' + p.name"
            :class="{ 'loading-row': loadingParam === p.device_id + '_' + p.idx }">
            <td>
              <div class="param-name">{{ p.name }}</div>
              <div class="param-device">{{ p.device_name }}</div>
            </td>
            <td class="param-type">{{ p.type }}</td>
            <td class="mono">{{ p.address_hex }}</td>
            <td class="param-val">{{ cacheGet(p.device_id, p.idx) }}</td>
            <td class="actions-cell">
              <button class="sm" @click="readOne(p)"
                :disabled="!p.connected || loadingParam === p.device_id + '_' + p.idx">Read</button>
              <button v-if="isMatrix(p)" class="sm" @click="openMatrix(p)" :disabled="!p.connected">Edit Matrix</button>
              <button v-else-if="p.connected" class="sm" @click="openWrite(p)">Write</button>
              <button class="sm" @click="openDetail(p)">Info</button>
              <button class="sm danger" @click="removeSelected(p)">✕</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- Write scalar modal -->
    <div v-if="writeTarget" class="modal-overlay" @click.self="writeTarget = null">
      <div class="modal">
        <h2>Write: {{ writeTarget.name }}</h2>
        <div class="field-label">Current: <span class="bright">{{ cacheGet(writeTarget.device_id, writeTarget.idx) }}</span></div>
        <input v-model="writeValue" class="inp full" placeholder="New value" @keyup.enter="confirmWrite" style="margin:10px 0" />
        <div class="modal-actions">
          <button @click="confirmWrite" :disabled="!writeValue || writing">{{ writing ? 'Writing…' : 'Write' }}</button>
          <button @click="writeTarget = null" class="warn">Cancel</button>
        </div>
        <div v-if="writeMsg" class="msg" :class="writeErr ? 'err' : ''">{{ writeMsg }}</div>
      </div>
    </div>

    <!-- Matrix editor modal -->
    <div v-if="matrixTarget" class="modal-overlay" @click.self="matrixTarget = null">
      <div class="modal modal-wide">
        <h2>Matrix Editor: {{ matrixTarget.name }}</h2>
        <div v-if="matrixLoading" class="loading-txt">Loading matrix data…</div>
        <template v-else>
          <div class="matrix-info">{{ matrixRows }}×{{ matrixCols }} — {{ matrixTarget.type }}</div>
          <div class="matrix-scroll">
            <table class="matrix-table">
              <tbody>
                <tr v-for="(row, ri) in matrixData" :key="ri">
                  <td class="row-idx">{{ ri }}</td>
                  <td v-for="(cell, ci) in row" :key="ci">
                    <input class="matrix-cell" :value="cell"
                      @change="e => setCellValue(ri, ci, e.target.value)"
                      @blur="e => writeCellBlur(ri, ci, e.target.value)" />
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </template>
        <div class="modal-actions">
          <button @click="matrixTarget = null" class="warn">Close</button>
        </div>
        <div v-if="matrixMsg" class="msg" :class="matrixErr ? 'err' : ''">{{ matrixMsg }}</div>
      </div>
    </div>

    <!-- Detail modal -->
    <div v-if="detailTarget" class="modal-overlay" @click.self="detailTarget = null">
      <div class="modal">
        <h2>Parameter Info</h2>
        <div v-if="detailLoading" class="loading-txt">Loading…</div>
        <table v-else-if="detailData" class="detail-table">
          <tr><td class="dl">Name</td><td>{{ detailData.name }}</td></tr>
          <tr><td class="dl">Type</td><td>{{ detailData.type }}</td></tr>
          <tr><td class="dl">Data Type</td><td>{{ detailData.data_type }}</td></tr>
          <tr><td class="dl">Address</td><td class="mono">{{ detailData.address_hex }}</td></tr>
          <tr><td class="dl">Unit</td><td>{{ detailData.unit || '—' }}</td></tr>
          <tr><td class="dl">Dimensions</td><td>{{ detailData.rows }}×{{ detailData.cols }}</td></tr>
          <tr><td class="dl">Description</td><td>{{ detailData.description || '—' }}</td></tr>
          <tr><td class="dl">CompuMethod</td><td>{{ detailData.compu_method_type || '—' }}</td></tr>
        </table>
        <div class="modal-actions">
          <button @click="detailTarget = null" class="warn">Close</button>
        </div>
      </div>
    </div>

    <!-- Add Variables modal -->
    <div v-if="showAddModal" class="modal-overlay" @click.self="showAddModal = false">
      <div class="modal modal-wide">
        <h2>Add Variables</h2>
        <div class="add-toolbar">
          <input v-model="addSearch" placeholder="Filter…" class="inp" style="flex:1" />
          <button @click="selectAllFiltered">Select All</button>
          <button @click="deselectAllFiltered" class="warn">Deselect All</button>
          <span class="count-info">{{ pendingKeys.size }} selected</span>
        </div>
        <div class="add-table-wrap">
          <table class="add-table">
            <thead>
              <tr><th></th><th>Name</th><th>Device</th><th>Type</th><th>Address</th></tr>
            </thead>
            <tbody>
              <tr v-for="p in filteredAddParams" :key="p.device_id + ':' + p.name"
                :class="{ 'in-sel': pendingKeys.has(p.device_id + ':' + p.name) }"
                @click="togglePending(p)">
                <td><input type="checkbox" :checked="pendingKeys.has(p.device_id + ':' + p.name)"
                  @change="togglePending(p)" @click.stop /></td>
                <td class="add-name">{{ p.name }}</td>
                <td class="add-device">{{ p.device_name }}</td>
                <td class="add-type">{{ p.type }}</td>
                <td class="mono">{{ p.address_hex }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div class="modal-actions">
          <button @click="applyAdd">Apply ({{ pendingKeys.size }} selected)</button>
          <button @click="showAddModal = false" class="warn">Cancel</button>
        </div>
      </div>
    </div>

    <div v-if="globalMsg" class="msg err">{{ globalMsg }}</div>
  </div>
</template>

<script setup>
import { ref, reactive, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const LS_KEY = 'cal_selected'

const allParams = ref([])
const connectedDevices = ref([])
const selectedKeys = ref(new Set(JSON.parse(localStorage.getItem(LS_KEY) || '[]')))
const cacheMap = reactive({})
const search = ref('')
const deviceFilter = ref('')
const readingAll = ref(false)
const readAllProgress = ref(0)
const loadingParam = ref('')
const pageModes = reactive({})
const togglingPage = ref('')
const globalMsg = ref('')

const writeTarget = ref(null)
const writeValue = ref('')
const writing = ref(false)
const writeMsg = ref('')
const writeErr = ref(false)

const matrixTarget = ref(null)
const matrixData = ref([])
const matrixRows = ref(0)
const matrixCols = ref(0)
const matrixLoading = ref(false)
const matrixMsg = ref('')
const matrixErr = ref(false)

const detailTarget = ref(null)
const detailData = ref(null)
const detailLoading = ref(false)

const showAddModal = ref(false)
const addSearch = ref('')
const pendingKeys = ref(new Set())

function cacheKey(deviceId, idx) { return `${deviceId}_${idx}` }
function cacheGet(deviceId, idx) { return cacheMap[cacheKey(deviceId, idx)] ?? '—' }
function cacheSet(deviceId, idx, val) { cacheMap[cacheKey(deviceId, idx)] = val }

function isMatrix(p) {
  const t = (p.type || '').toLowerCase()
  return t.includes('array') || t.includes('matrix') || t.includes('map') || (p.cols && p.cols > 1)
}

const displayedParams = computed(() => {
  let list = allParams.value.filter(p => selectedKeys.value.has(p.device_id + ':' + p.name))
  if (deviceFilter.value) list = list.filter(p => p.device_id === deviceFilter.value)
  if (search.value) {
    const q = search.value.toLowerCase()
    list = list.filter(p => p.name.toLowerCase().includes(q))
  }
  return list
})

const filteredAddParams = computed(() => {
  if (!addSearch.value) return allParams.value
  const q = addSearch.value.toLowerCase()
  return allParams.value.filter(p =>
    p.name.toLowerCase().includes(q) || (p.device_name || '').toLowerCase().includes(q))
})

function saveSelectedKeys() {
  localStorage.setItem(LS_KEY, JSON.stringify([...selectedKeys.value]))
}

async function loadAll() {
  try {
    const r = await invoke('cal_available')
    allParams.value = r.params ?? []
    globalMsg.value = ''
  } catch (e) { globalMsg.value = String(e) }
}

async function loadDevices() {
  try {
    const r = await invoke('list_devices')
    connectedDevices.value = (r.devices ?? []).filter(d => d.connected)
    try {
      const pm = await invoke('get_page_mode')
      const byId = Object.fromEntries((pm.devices ?? []).map(d => [d.device_id, d.page_mode ?? '—']))
      for (const d of connectedDevices.value) {
        pageModes[d.id] = byId[d.id] ?? '?'
      }
    } catch {
      for (const d of connectedDevices.value) {
        pageModes[d.id] = '?'
      }
    }
  } catch { }
}

async function togglePage(deviceId) {
  togglingPage.value = deviceId
  const next = pageModes[deviceId] === 'Working' ? 'Reference' : 'Working'
  try {
    await invoke('switch_page_mode', { deviceId, mode: next })
    pageModes[deviceId] = next
  } catch (e) { globalMsg.value = String(e) }
  finally { togglingPage.value = '' }
}

async function readOne(p) {
  loadingParam.value = p.device_id + '_' + p.idx
  try {
    const r = await invoke('cal_read', { deviceId: p.device_id, idx: p.idx })
    cacheSet(p.device_id, p.idx, r.value ?? '—')
  } catch (e) { globalMsg.value = String(e) }
  finally { loadingParam.value = '' }
}

async function readAll() {
  readingAll.value = true; readAllProgress.value = 0
  const list = displayedParams.value.filter(p => p.connected)
  for (const p of list) {
    await readOne(p)
    readAllProgress.value++
    await new Promise(r => setTimeout(r, 30))
  }
  readingAll.value = false
}

function removeSelected(p) {
  const ns = new Set(selectedKeys.value)
  ns.delete(p.device_id + ':' + p.name)
  selectedKeys.value = ns
  saveSelectedKeys()
}

function openWrite(p) {
  writeTarget.value = p
  writeValue.value = ''
  writeMsg.value = ''
  writeErr.value = false
}

async function confirmWrite() {
  if (!writeTarget.value || !writeValue.value) return
  writing.value = true; writeMsg.value = ''; writeErr.value = false
  try {
    await invoke('cal_write', {
      deviceId: writeTarget.value.device_id,
      idx: writeTarget.value.idx,
      value: writeValue.value
    })
    writeMsg.value = 'Written successfully'
    await readOne(writeTarget.value)
  } catch (e) { writeMsg.value = String(e); writeErr.value = true }
  finally { writing.value = false }
}

async function openMatrix(p) {
  matrixTarget.value = p
  matrixData.value = []
  matrixRows.value = 0; matrixCols.value = 0
  matrixMsg.value = ''; matrixErr.value = false
  matrixLoading.value = true
  try {
    const [readRes, detailRes] = await Promise.all([
      invoke('cal_read', { deviceId: p.device_id, idx: p.idx }),
      invoke('cal_detail', { deviceId: p.device_id, idx: p.idx })
    ])
    matrixRows.value = detailRes.rows ?? 1
    matrixCols.value = detailRes.cols ?? 1
    const val = readRes.value
    if (Array.isArray(val) && Array.isArray(val[0])) {
      matrixData.value = val.map(row => [...row])
    } else if (Array.isArray(val)) {
      const rows = matrixRows.value, cols = matrixCols.value
      const data = []
      for (let r = 0; r < rows; r++) data.push(val.slice(r * cols, r * cols + cols))
      matrixData.value = data
    } else {
      matrixData.value = [[val]]
    }
  } catch (e) { matrixMsg.value = String(e); matrixErr.value = true }
  finally { matrixLoading.value = false }
}

function setCellValue(ri, ci, val) {
  if (matrixData.value[ri]) matrixData.value[ri][ci] = val
}

async function writeCellBlur(ri, ci, val) {
  if (!matrixTarget.value) return
  try {
    await invoke('cal_write_element', {
      deviceId: matrixTarget.value.device_id,
      idx: matrixTarget.value.idx,
      row: ri, col: ci, value: val
    })
    matrixMsg.value = `Cell [${ri},${ci}] written`
    matrixErr.value = false
  } catch (e) { matrixMsg.value = String(e); matrixErr.value = true }
}

async function openDetail(p) {
  detailTarget.value = p
  detailData.value = null
  detailLoading.value = true
  try {
    detailData.value = await invoke('cal_detail', { deviceId: p.device_id, idx: p.idx })
  } catch (e) { globalMsg.value = String(e) }
  finally { detailLoading.value = false }
}

function openAddModal() {
  pendingKeys.value = new Set(selectedKeys.value)
  addSearch.value = ''
  showAddModal.value = true
}

function togglePending(p) {
  const k = p.device_id + ':' + p.name
  const ns = new Set(pendingKeys.value)
  if (ns.has(k)) ns.delete(k); else ns.add(k)
  pendingKeys.value = ns
}

function selectAllFiltered() {
  const ns = new Set(pendingKeys.value)
  filteredAddParams.value.forEach(p => ns.add(p.device_id + ':' + p.name))
  pendingKeys.value = ns
}

function deselectAllFiltered() {
  const ns = new Set(pendingKeys.value)
  filteredAddParams.value.forEach(p => ns.delete(p.device_id + ':' + p.name))
  pendingKeys.value = ns
}

function applyAdd() {
  selectedKeys.value = new Set(pendingKeys.value)
  saveSelectedKeys()
  showAddModal.value = false
}

onMounted(async () => { await Promise.all([loadDevices(), loadAll()]) })
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 14px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 1rem; margin-bottom: 10px; }
.toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
.count { font-size: 0.8rem; color: #555; margin-bottom: 8px; }
.count-info { font-size: 0.8rem; color: #555; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: Consolas, monospace; }
.sel { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: Consolas, monospace; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: Consolas, monospace; font-size: 0.9rem; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
button.sm { padding: 2px 7px; font-size: 0.8rem; }
button.warn { border-color: #ffcc00; color: #ffcc00; }
button.warn:hover:not(:disabled) { background: #1a1100; }
button.danger { border-color: #ff4444; color: #ff4444; }
button.danger:hover:not(:disabled) { background: #1a0000; }
.page-mode-bar { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; margin-bottom: 10px; padding: 6px 10px; background: #0d0d0d; border: 1px solid #003311; }
.pm-label { color: #006622; font-size: 0.8rem; }
.pm-device { display: flex; align-items: center; gap: 5px; }
.pm-name { color: #00cc44; font-size: 0.85rem; }
.pm-btn { padding: 2px 8px; font-size: 0.8rem; }
.pm-btn.working { border-color: #00ff66; color: #00ff66; }
.pm-btn.reference { border-color: #5599ff; color: #5599ff; }
.table-wrap { overflow-x: auto; }
.param-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
.param-table th { color: #006622; text-align: left; padding: 5px 8px; border-bottom: 1px solid #003311; }
.param-table td { padding: 5px 8px; border-bottom: 1px solid #001a09; vertical-align: middle; }
.param-table tr.loading-row td { opacity: 0.5; }
.param-name { color: #00ff66; font-weight: bold; }
.param-device { color: #555; font-size: 0.78rem; }
.param-type { color: #666; }
.param-val { font-family: Consolas, monospace; color: #00cc44; max-width: 180px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.actions-cell { display: flex; gap: 4px; flex-wrap: wrap; }
.mono { font-family: Consolas, monospace; }
.empty { color: #555; padding: 24px 0; }
.modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.75); display: flex; align-items: center; justify-content: center; z-index: 100; }
.modal { background: #0d0d0d; border: 1px solid #003311; padding: 20px; min-width: 420px; max-height: 85vh; overflow-y: auto; }
.modal.modal-wide { min-width: 640px; max-width: 90vw; }
.field-label { color: #006622; font-size: 0.85rem; margin-bottom: 4px; }
.bright { color: #00ff66; }
.inp.full { width: 100%; box-sizing: border-box; }
.modal-actions { display: flex; gap: 8px; margin-top: 12px; }
.loading-txt { color: #555; padding: 12px 0; }
.matrix-info { color: #006622; font-size: 0.8rem; margin-bottom: 8px; }
.matrix-scroll { overflow: auto; max-height: 400px; }
.matrix-table { border-collapse: collapse; }
.matrix-table td { padding: 2px; }
.row-idx { color: #444; font-size: 0.75rem; padding-right: 6px; font-family: Consolas, monospace; min-width: 24px; }
.matrix-cell { width: 70px; background: #0a0a0a; border: 1px solid #002208; color: #00cc44; padding: 3px 5px; font-family: Consolas, monospace; font-size: 0.82rem; text-align: right; }
.matrix-cell:focus { border-color: #00ff66; outline: none; background: #001a09; }
.detail-table { width: 100%; border-collapse: collapse; font-size: 0.88rem; }
.detail-table tr { border-bottom: 1px solid #001a09; }
.detail-table td { padding: 6px 8px; }
.dl { color: #006622; font-size: 0.8rem; white-space: nowrap; width: 110px; }
.add-toolbar { display: flex; gap: 8px; align-items: center; margin-bottom: 10px; }
.add-table-wrap { max-height: 380px; overflow-y: auto; border: 1px solid #003311; }
.add-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
.add-table th { color: #006622; text-align: left; padding: 5px 8px; border-bottom: 1px solid #003311; position: sticky; top: 0; background: #0d0d0d; }
.add-table td { padding: 4px 8px; border-bottom: 1px solid #001a09; }
.add-table tr { cursor: pointer; }
.add-table tr:hover td { background: #001a09; }
.add-table tr.in-sel td { background: #001a09; }
.add-table input[type=checkbox] { accent-color: #00cc44; }
.add-name { color: #00ff66; }
.add-device { color: #555; font-size: 0.8rem; }
.add-type { color: #666; }
.msg { margin-top: 8px; font-size: 0.85rem; color: #00cc44; }
.msg.err { color: #ff4444; }
</style>
