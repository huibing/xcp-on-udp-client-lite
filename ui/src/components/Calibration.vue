<template>
  <div class="page">
    <h1>Calibration</h1>

    <!-- Toolbar -->
    <div class="toolbar">
      <input v-model="search" placeholder="Search parameters…" class="inp" @input="offset = 0; load()" />
      <select v-model="selectedDevice" @change="offset = 0; load()" class="sel">
        <option value="">All devices</option>
        <option v-for="d in devices" :key="d.id" :value="d.id">{{ d.name }}</option>
      </select>
      <button @click="load">↻</button>
      <span class="page-mode">Page: <b>{{ pageMode }}</b></span>
      <button v-if="selectedDevice" @click="togglePage">Switch Page</button>
    </div>

    <div class="count">{{ filtered.length }} parameter(s)</div>

    <div class="param-list">
      <div v-for="p in filtered" :key="p.device_id + '_' + p.idx" class="param-row" @click="select(p)">
        <div class="param-main">
          <span class="param-name">{{ p.name }}</span>
          <span class="param-type">{{ p.type }}</span>
          <span class="param-unit">{{ p.unit || '' }}</span>
        </div>
        <div class="param-sub">{{ p.device_name }} · {{ p.address_hex }}</div>
        <div v-if="p.connected" class="param-val">
          {{ cachedValue(p) }}
          <button @click.stop="readParam(p)" class="sm">Read</button>
        </div>
      </div>
    </div>

    <!-- Detail / Write panel -->
    <div v-if="selected" class="detail-panel">
      <div class="detail-header">
        <b>{{ selected.name }}</b>
        <button @click="selected = null" class="sm">✕</button>
      </div>
      <div class="detail-row"><span>Type</span><span>{{ selected.type }}</span></div>
      <div class="detail-row"><span>Address</span><span>{{ selected.address_hex }}</span></div>
      <div class="detail-row"><span>Unit</span><span>{{ selected.unit || '—' }}</span></div>
      <div class="detail-row"><span>Device</span><span>{{ selected.device_name }}</span></div>
      <div class="detail-row"><span>Value</span><span class="val">{{ detailValue }}</span></div>
      <div class="write-row" v-if="selected.connected">
        <input v-model="writeValue" placeholder="New value" class="inp" @keyup.enter="writeParam" />
        <button @click="writeParam">Write</button>
        <button @click="readSelected">Read</button>
      </div>
      <div v-if="writeMsg" class="msg" :class="writeErr ? 'err' : ''">{{ writeMsg }}</div>
    </div>

    <div v-if="msg" class="msg err">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const params = ref([])
const devices = ref([])
const search = ref('')
const selectedDevice = ref('')
const offset = ref(0)
const selected = ref(null)
const detailValue = ref('—')
const writeValue = ref('')
const writeMsg = ref('')
const writeErr = ref(false)
const msg = ref('')
const pageMode = ref('—')
const cache = ref({})  // device_id+idx -> display string

const filtered = computed(() => {
  let list = params.value
  if (selectedDevice.value) list = list.filter(p => p.device_id === selectedDevice.value)
  if (search.value) list = list.filter(p => p.name.toLowerCase().includes(search.value.toLowerCase()))
  return list
})

function cachedValue(p) {
  return cache.value[p.device_id + '_' + p.idx] ?? '—'
}

async function load() {
  try {
    const r = await invoke('cal_available')
    params.value = r.params
    msg.value = ''
  } catch (e) { msg.value = String(e) }
}

async function loadDevices() {
  const r = await invoke('list_devices')
  devices.value = r.devices.filter(d => d.connected)
}

async function loadPageMode() {
  if (!selectedDevice.value) { pageMode.value = '—'; return }
  try {
    const r = await invoke('get_page_mode', { deviceId: selectedDevice.value })
    pageMode.value = r.mode ?? '—'
  } catch { pageMode.value = '?' }
}

async function togglePage() {
  if (!selectedDevice.value) return
  const next = pageMode.value === 'Working' ? 'Reference' : 'Working'
  try {
    await invoke('switch_page_mode', { deviceId: selectedDevice.value, mode: next })
    await loadPageMode()
  } catch (e) { msg.value = String(e) }
}

async function readParam(p) {
  try {
    const r = await invoke('cal_read', { deviceId: p.device_id, idx: p.idx })
    cache.value[p.device_id + '_' + p.idx] = r.display ?? JSON.stringify(r.value)
    if (selected.value?.device_id === p.device_id && selected.value?.idx === p.idx) {
      detailValue.value = cache.value[p.device_id + '_' + p.idx]
    }
  } catch (e) { msg.value = String(e) }
}

function select(p) {
  selected.value = p
  detailValue.value = cachedValue(p)
  writeValue.value = ''
  writeMsg.value = ''
}

async function readSelected() {
  if (!selected.value) return
  await readParam(selected.value)
}

async function writeParam() {
  if (!selected.value || !writeValue.value) return
  writeErr.value = false; writeMsg.value = ''
  try {
    await invoke('cal_write', { deviceId: selected.value.device_id, idx: selected.value.idx, value: writeValue.value })
    writeMsg.value = 'Written'
    await readSelected()
  } catch (e) { writeMsg.value = String(e); writeErr.value = true }
}

onMounted(async () => { await loadDevices(); await load(); await loadPageMode() })
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 12px; font-size: 1.4rem; }

.toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; font-size: 0.9rem; }
.sel { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: inherit; }
button:hover { background: #003311; }
button.sm { padding: 2px 7px; font-size: 0.8rem; }
.page-mode { color: #006622; font-size: 0.85rem; }

.count { color: #006622; font-size: 0.8rem; margin-bottom: 8px; }

.param-list { max-height: 360px; overflow-y: auto; border: 1px solid #001a09; }
.param-row { padding: 7px 10px; border-bottom: 1px solid #001a09; cursor: pointer; }
.param-row:hover { background: #0d0d0d; }
.param-main { display: flex; gap: 12px; align-items: baseline; }
.param-name { color: #00ff66; font-weight: bold; }
.param-type { color: #006622; font-size: 0.8rem; }
.param-unit { color: #555; font-size: 0.8rem; }
.param-sub { color: #444; font-size: 0.75rem; margin-top: 2px; }
.param-val { display: flex; gap: 8px; align-items: center; margin-top: 3px; font-size: 0.85rem; }

.detail-panel { margin-top: 16px; background: #0d0d0d; border: 1px solid #003311; padding: 14px; }
.detail-header { display: flex; justify-content: space-between; margin-bottom: 10px; color: #00ff66; }
.detail-row { display: flex; justify-content: space-between; padding: 4px 0; border-bottom: 1px solid #001a09; font-size: 0.9rem; }
.detail-row span:first-child { color: #006622; }
.val { color: #00ff66; }
.write-row { display: flex; gap: 8px; margin-top: 10px; }
.msg { margin-top: 6px; font-size: 0.85rem; color: #00cc44; }
.msg.err { color: #ff4444; }
</style>
