<template>
  <div class="page">
    <h1>Device Manager</h1>

    <!-- Add Device Form -->
    <section class="card">
      <h2>Add Device</h2>
      <div class="row">
        <input v-model="newName" placeholder="Device name" class="inp" />
        <input v-model="newAddr" placeholder="IP:port (e.g. 192.168.1.1:5555)" class="inp wide" />
        <button @click="addDevice">Add</button>
      </div>
    </section>

    <!-- Device List -->
    <section class="card">
      <h2>Devices</h2>
      <div v-if="devices.length === 0" class="empty">No devices. Add one above.</div>
      <div v-for="d in devices" :key="d.id" class="dev-row">
        <div class="dev-info">
          <span class="dev-name">{{ d.name }}</span>
          <span class="dev-addr">{{ d.address }}</span>
          <span :class="['status', d.connected ? 'ok' : 'off']">{{ d.connected ? '● CONNECTED' : '○ offline' }}</span>
          <span v-if="d.a2l_loaded" class="a2l-badge">A2L ✓</span>
        </div>
        <div class="dev-actions">
          <button v-if="!d.connected" @click="connect(d.id)" :disabled="loading === d.id">Connect</button>
          <button v-else @click="disconnect(d.id)" :disabled="loading === d.id" class="warn">Disconnect</button>
          <button @click="loadA2L(d.id)" :disabled="!d.connected" title="Load A2L file">Load A2L</button>
          <button @click="remove(d.id)" class="danger" :disabled="d.connected">Remove</button>
        </div>
      </div>
    </section>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>

    <!-- A2L file path input dialog (inline) -->
    <div v-if="a2lTarget" class="overlay">
      <div class="dialog">
        <h3>Load A2L for {{ a2lTarget }}</h3>
        <input v-model="a2lPath" placeholder="Full path to .a2l file" class="inp wide" @keyup.enter="confirmA2L" />
        <div class="row">
          <button @click="confirmA2L">Load</button>
          <button @click="a2lTarget = ''" class="warn">Cancel</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const devices = ref([])
const loading = ref('')
const msg = ref('')
const msgErr = ref(false)
const newName = ref('')
const newAddr = ref('')
const a2lTarget = ref('')
const a2lPath = ref('')

async function refresh() {
  const r = await invoke('list_devices')
  devices.value = r.devices
}

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 4000)
}

async function addDevice() {
  if (!newName.value || !newAddr.value) return setMsg('Name and address required', true)
  try {
    await invoke('add_device', { name: newName.value, address: newAddr.value })
    newName.value = ''; newAddr.value = ''
    await refresh(); setMsg('Device added')
  } catch (e) { setMsg(String(e), true) }
}

async function connect(id) {
  loading.value = id
  try {
    const r = await invoke('connect_device', { deviceId: id })
    if (r.success) { setMsg('Connected'); await refresh() }
    else setMsg(r.error || 'Connection failed', true)
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = '' }
}

async function disconnect(id) {
  loading.value = id
  try {
    await invoke('disconnect_device', { deviceId: id })
    setMsg('Disconnected'); await refresh()
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = '' }
}

async function remove(id) {
  try {
    await invoke('remove_device', { deviceId: id })
    await refresh(); setMsg('Removed')
  } catch (e) { setMsg(String(e), true) }
}

function loadA2L(id) {
  a2lTarget.value = id
  a2lPath.value = ''
}

async function confirmA2L() {
  if (!a2lPath.value) return
  try {
    await invoke('load_a2l', { deviceId: a2lTarget.value, path: a2lPath.value })
    a2lTarget.value = ''; await refresh(); setMsg('A2L loaded')
  } catch (e) { setMsg(String(e), true) }
}

onMounted(refresh)
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 16px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 1rem; margin-bottom: 10px; }

.card { background: #0d0d0d; border: 1px solid #003311; padding: 14px; border-radius: 4px; margin-bottom: 16px; }
.row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; font-size: 0.9rem; }
.inp.wide { flex: 1; min-width: 200px; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 12px; cursor: pointer; font-family: inherit; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
button.warn { border-color: #ffcc00; color: #ffcc00; }
button.warn:hover:not(:disabled) { background: #1a1100; }
button.danger { border-color: #ff4444; color: #ff4444; }
button.danger:hover:not(:disabled) { background: #1a0000; }

.dev-row { display: flex; justify-content: space-between; align-items: center; padding: 8px 0; border-bottom: 1px solid #001a09; flex-wrap: wrap; gap: 8px; }
.dev-info { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
.dev-name { color: #00ff66; font-weight: bold; }
.dev-addr { color: #555; font-size: 0.85rem; }
.status.ok { color: #00ff66; font-size: 0.8rem; }
.status.off { color: #444; font-size: 0.8rem; }
.a2l-badge { background: #003311; color: #00cc44; padding: 1px 6px; font-size: 0.75rem; }
.dev-actions { display: flex; gap: 6px; }

.empty { color: #555; padding: 8px 0; }
.msg { margin-top: 8px; color: #00cc44; font-size: 0.85rem; }
.msg.err { color: #ff4444; }

.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 100; }
.dialog { background: #0d0d0d; border: 1px solid #00cc44; padding: 20px; min-width: 360px; }
.dialog h3 { color: #00ff66; margin-bottom: 12px; }
.dialog .row { margin-top: 12px; }
</style>
