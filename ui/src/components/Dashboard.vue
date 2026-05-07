<template>
  <div class="page">
    <div class="toolbar">
      <h1>Dashboard</h1>
      <div class="tb-actions">
        <button @click="refresh">↻ Refresh</button>
        <button @click="connectAll" :disabled="loading !== ''">Connect All</button>
        <button @click="disconnectAll" :disabled="connectedCount === 0 || loading !== ''" class="warn">Disconnect All</button>
      </div>
    </div>

    <div class="stats-row">
      <div class="stat-card">
        <div class="stat-label">Total Devices</div>
        <div class="stat-value">{{ devices.length }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">Connected</div>
        <div class="stat-value connected">{{ connectedCount }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">A2L Loaded</div>
        <div class="stat-value">{{ a2lLoaded }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">Offline</div>
        <div class="stat-value offline">{{ devices.length - connectedCount }}</div>
      </div>
    </div>

    <div v-if="devices.length === 0" class="empty">No devices configured. Go to Device Manager to add one.</div>

    <div class="device-grid">
      <div v-for="d in devices" :key="d.id" class="device-card" :class="{ connected: d.connected }">
        <div class="card-header">
          <span class="dev-name">{{ d.name }}</span>
          <span class="status-badge" :class="d.connected ? 'ok' : 'off'">{{ d.connected ? '● ON' : '○ off' }}</span>
        </div>
        <div class="dev-addr">{{ d.address }}</div>
        <div class="card-badges">
          <span v-if="d.a2l_loaded" class="badge a2l">A2L ✓</span>
          <span v-if="d.slave_id" class="badge slave">{{ d.slave_id.slice(0, 8) }}</span>
        </div>
        <div class="card-actions">
          <button v-if="!d.connected" @click="connectDevice(d.id)" :disabled="loading === d.id">
            {{ loading === d.id ? '…' : 'Connect' }}
          </button>
          <button v-else @click="disconnectDevice(d.id)" :disabled="loading === d.id" class="warn">
            {{ loading === d.id ? '…' : 'Disconnect' }}
          </button>
        </div>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const devices = ref([])
const loading = ref('')
const msg = ref('')
const msgErr = ref(false)
let timer = null

const connectedCount = computed(() => devices.value.filter(d => d.connected).length)
const a2lLoaded = computed(() => devices.value.filter(d => d.a2l_loaded).length)

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 5000)
}

async function refresh() {
  try {
    const r = await invoke('list_devices')
    devices.value = r.devices ?? []
  } catch (e) { setMsg(String(e), true) }
}

async function connectDevice(id) {
  loading.value = id
  try {
    const r = await invoke('connect_device', { deviceId: id })
    if (r.success) { await refresh(); setMsg(r.message || 'Connected') }
    else setMsg('Connection failed', true)
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = '' }
}

async function disconnectDevice(id) {
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
    setMsg(r.message || `Connected ${r.connected ?? 0} devices`, !r.success)
    await refresh()
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = '' }
}

async function disconnectAll() {
  loading.value = 'all'
  try {
    const connected = devices.value.filter(d => d.connected)
    for (const device of connected) {
      await invoke('disconnect_device', { deviceId: device.id })
    }
    await refresh(); setMsg('All devices disconnected')
  } catch (e) { setMsg(String(e), true) }
  finally { loading.value = '' }
}

onMounted(() => { refresh(); timer = setInterval(refresh, 10000) })
onUnmounted(() => { if (timer) clearInterval(timer) })
</script>

<style scoped>
.page { color: var(--exp-fg, #00cc44); }
.toolbar { display: flex; justify-content: space-between; align-items: center; margin-bottom: 16px; flex-wrap: wrap; gap: 8px; }
h1 { color: var(--exp-fg-bright, #00ff66); font-size: 1.4rem; margin: 0; }
.tb-actions { display: flex; gap: 8px; }

.stats-row { display: flex; gap: 16px; margin-bottom: 24px; flex-wrap: wrap; }
.stat-card { background: var(--exp-bg3, #0d0d0d); border: 1px solid var(--exp-border, #003311); padding: 12px 20px; min-width: 110px; }
.stat-label { color: var(--exp-fg-dim3, #006622); font-size: 0.75rem; text-transform: uppercase; margin-bottom: 4px; }
.stat-value { font-size: 1.8rem; color: var(--exp-fg, #00cc44); }
.stat-value.connected { color: var(--exp-fg-bright, #00ff66); }
.stat-value.offline { color: var(--exp-fg-dim, #555); }

.device-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; margin-bottom: 16px; }
.device-card { background: var(--exp-bg3, #0d0d0d); border: 1px solid var(--exp-border, #003311); padding: 12px; }
.device-card.connected { border-color: var(--exp-fg-dim3, #006622); }
.card-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 6px; }
.dev-name { color: var(--exp-fg-bright, #00ff66); font-weight: bold; font-size: 1rem; }
.status-badge { font-size: 0.75rem; padding: 2px 6px; border: 1px solid; }
.status-badge.ok { color: var(--exp-fg-bright, #00ff66); border-color: var(--exp-fg-dim3, #006622); }
.status-badge.off { color: var(--exp-fg-dim, #444); border-color: var(--exp-border2, #222); }
.dev-addr { color: var(--exp-fg-dim, #555); font-size: 0.8rem; font-family: Consolas, monospace; margin-bottom: 6px; }
.card-badges { display: flex; gap: 4px; margin-bottom: 8px; min-height: 20px; }
.badge { font-size: 0.72rem; padding: 1px 6px; border: 1px solid; }
.badge.a2l { color: var(--exp-fg, #00cc44); border-color: var(--exp-border, #003311); background: var(--exp-border3, #001a09); }
.badge.slave { color: var(--exp-fg-dim, #555); border-color: var(--exp-border2, #222); font-family: Consolas, monospace; }
.card-actions { display: flex; }

button { background: var(--exp-border3, #001a09); border: 1px solid var(--exp-fg, #00cc44); color: var(--exp-fg, #00cc44); padding: 5px 10px; cursor: pointer; font-family: Consolas, monospace; font-size: 0.9rem; }
button:hover:not(:disabled) { background: var(--exp-hover, #003311); }
button:disabled { opacity: 0.35; cursor: default; }
button.warn { border-color: #ffcc00; color: #ffcc00; }
button.warn:hover:not(:disabled) { background: #1a1100; }

.empty { color: var(--exp-fg-dim, #555); padding: 24px 0; }
.msg { margin-top: 8px; color: var(--exp-fg, #00cc44); font-size: 0.85rem; }
.msg.err { color: #ff4444; }
</style>
