<template>
  <div class="page">
    <h1>Device Manager</h1>

    <div class="card">
      <h2>Add Device</h2>
      <div class="row">
        <input v-model="newName" placeholder="Device name" class="inp" @keyup.enter="addDevice" />
        <input v-model="newAddr" placeholder="IP:port (e.g. 192.168.1.1:5555)" class="inp wide" @keyup.enter="addDevice" />
        <button @click="addDevice" :disabled="!newName || !newAddr">+ Add</button>
      </div>
    </div>

    <div class="toolbar">
      <button @click="loadFromConfig">⬆ Load from Config</button>
      <button @click="connectAll" :disabled="loading !== ''">Connect All</button>
      <button @click="disconnectAll" :disabled="connectedCount === 0 || loading !== ''" class="warn">Disconnect All</button>
      <button @click="refresh">↻ Refresh</button>
    </div>

    <div v-if="toast" class="toast">{{ toast }}</div>

    <div class="card">
      <h2>Devices ({{ devices.length }})</h2>
      <div v-if="devices.length === 0" class="empty">No devices configured.</div>
      <div v-for="d in devices" :key="d.id" class="dev-row">
        <div class="dev-info">
          <span v-if="editingId !== d.id" class="dev-name" @click="startEdit(d)" title="Click to rename">{{ d.name }}</span>
          <input v-else class="inp rename-inp" v-model="editingValue"
            @blur="commitRename(d.id)"
            @keyup.enter="commitRename(d.id)"
            @keyup.escape="editingId = ''"
            ref="renameInputRef" />
          <span class="dev-addr mono">{{ d.address }}</span>
          <span class="status-badge" :class="d.connected ? 'ok' : 'off'">
            {{ d.connected ? '● CONNECTED' : '○ offline' }}
          </span>
          <span v-if="d.a2l_loaded" class="badge a2l">A2L ✓</span>
        </div>
        <div class="dev-actions">
          <button v-if="!d.connected" @click="connect(d.id)" :disabled="loading === d.id">
            {{ loading === d.id ? '…' : 'Connect' }}
          </button>
          <button v-else @click="disconnect(d.id)" :disabled="loading === d.id" class="warn">
            {{ loading === d.id ? '…' : 'Disconnect' }}
          </button>
          <button @click="loadA2L(d)" :disabled="!d.connected || loading === d.id + '_a2l'">
            {{ loading === d.id + '_a2l' ? '…' : 'Load A2L' }}
          </button>
          <button @click="remove(d.id)" class="danger" :disabled="d.connected || loading === d.id">Remove</button>
        </div>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const devices = ref([])
const loading = ref('')
const msg = ref('')
const msgErr = ref(false)
const toast = ref('')
const newName = ref('')
const newAddr = ref('')
const editingId = ref('')
const editingValue = ref('')
const renameInputRef = ref(null)
let toastTimer = null

const connectedCount = computed(() => devices.value.filter(d => d.connected).length)

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 5000)
}

function showToast(text) {
  toast.value = text
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => toast.value = '', 4000)
}

async function refresh() {
  try {
    const r = await invoke('list_devices')
    devices.value = r.devices ?? []
  } catch (e) { setMsg(String(e), true) }
}

async function addDevice() {
  if (!newName.value || !newAddr.value) return setMsg('Name and address required', true)
  try {
    await invoke('add_device', { name: newName.value, address: newAddr.value })
    newName.value = ''; newAddr.value = ''
    await refresh(); setMsg('Device added')
  } catch (e) { setMsg(String(e), true) }
}

async function loadFromConfig() {
  try {
    const r = await invoke('load_devices_from_config')
    await refresh(); setMsg(`Loaded ${r.loaded ?? 0} device(s) from config`)
  } catch (e) { setMsg(String(e), true) }
}

async function connect(id) {
  loading.value = id
  try {
    const r = await invoke('connect_device', { deviceId: id })
    if (r.ok) { await refresh(); setMsg('Connected') }
    else setMsg('Connection failed', true)
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = '' }
}

async function disconnect(id) {
  loading.value = id
  try {
    await invoke('disconnect_device', { deviceId: id })
    await refresh(); setMsg('Disconnected')
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = '' }
}

async function connectAll() {
  loading.value = 'all'
  try {
    const r = await invoke('connect_all_devices')
    const ok = (r.results ?? []).filter(x => !x.error).length
    await refresh(); setMsg(`Connected ${ok}/${(r.results ?? []).length}`)
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = '' }
}

async function disconnectAll() {
  loading.value = 'all'
  try {
    await invoke('disconnect_all_devices')
    await refresh(); setMsg('All disconnected')
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = '' }
}

async function remove(id) {
  try {
    await invoke('remove_device', { deviceId: id })
    await refresh(); setMsg('Removed')
  } catch (e) { setMsg(String(e), true) }
}

async function loadA2L(d) {
  try {
    const r = await invoke('open_file_dialog', {
      title: 'Select A2L File',
      filters: [{ name: 'A2L Files', extensions: ['a2l', 'A2L'] }]
    })
    if (!r.path) return
    loading.value = d.id + '_a2l'
    const res = await invoke('load_a2l', { deviceId: d.id, filePath: r.path })
    if (res.ok) {
      showToast(`A2L loaded: ${res.param_count} params, ${res.measurement_count} measurements`)
      await refresh()
    } else {
      setMsg('A2L load failed', true)
    }
  } catch (e) { setMsg(String(e), true) }
  finally { if (loading.value.startsWith(d.id)) loading.value = '' }
}

function startEdit(d) {
  editingId.value = d.id
  editingValue.value = d.name
  nextTick(() => { renameInputRef.value?.focus?.() })
}

async function commitRename(id) {
  if (!editingValue.value.trim()) { editingId.value = ''; return }
  try {
    await invoke('rename_device', { deviceId: id, name: editingValue.value.trim() })
    await refresh()
  } catch (e) { setMsg(String(e), true) }
  finally { editingId.value = '' }
}

onMounted(refresh)
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 16px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 1rem; margin-bottom: 10px; }
.card { background: #0d0d0d; border: 1px solid #003311; padding: 14px; margin-bottom: 16px; }
.toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 16px; }
.row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: Consolas, monospace; font-size: 0.9rem; }
.inp.wide { flex: 1; min-width: 200px; }
.rename-inp { width: 140px; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: Consolas, monospace; font-size: 0.9rem; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
button.warn { border-color: #ffcc00; color: #ffcc00; }
button.warn:hover:not(:disabled) { background: #1a1100; }
button.danger { border-color: #ff4444; color: #ff4444; }
button.danger:hover:not(:disabled) { background: #1a0000; }
.dev-row { display: flex; justify-content: space-between; align-items: center; padding: 8px 0; border-bottom: 1px solid #001a09; flex-wrap: wrap; gap: 8px; }
.dev-info { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; flex: 1; }
.dev-name { color: #00ff66; font-weight: bold; cursor: pointer; }
.dev-name:hover { text-decoration: underline; }
.dev-addr { color: #555; font-size: 0.85rem; }
.mono { font-family: Consolas, monospace; }
.status-badge { font-size: 0.75rem; padding: 2px 5px; border: 1px solid; }
.status-badge.ok { color: #00ff66; border-color: #006622; }
.status-badge.off { color: #444; border-color: #222; }
.badge { font-size: 0.72rem; padding: 1px 5px; border: 1px solid; }
.badge.a2l { color: #00cc44; border-color: #003311; background: #001a09; }
.dev-actions { display: flex; gap: 6px; flex-wrap: wrap; }
.empty { color: #555; padding: 8px 0; }
.msg { margin-top: 8px; color: #00cc44; font-size: 0.85rem; }
.msg.err { color: #ff4444; }
.toast { position: fixed; top: 20px; right: 20px; background: #001a09; border: 1px solid #00ff66; color: #00ff66; padding: 10px 18px; font-size: 0.9rem; z-index: 200; font-family: Consolas, monospace; }
</style>
