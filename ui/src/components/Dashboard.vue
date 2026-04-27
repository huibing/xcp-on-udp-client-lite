<template>
  <div class="page">
    <h1>Dashboard</h1>
    <div class="stats-row">
      <div class="stat-card">
        <div class="stat-label">Devices</div>
        <div class="stat-value">{{ devices.length }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">Connected</div>
        <div class="stat-value connected">{{ connected }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">A2L Loaded</div>
        <div class="stat-value">{{ a2lLoaded }}</div>
      </div>
    </div>

    <div v-if="devices.length === 0" class="empty">No devices configured. Go to Devices to add one.</div>

    <table v-else class="dev-table">
      <thead>
        <tr>
          <th>Name</th>
          <th>Address</th>
          <th>Status</th>
          <th>A2L</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="d in devices" :key="d.id">
          <td>{{ d.name }}</td>
          <td>{{ d.address }}</td>
          <td :class="d.connected ? 'ok' : 'off'">{{ d.connected ? '● CONNECTED' : '○ offline' }}</td>
          <td>{{ d.a2l_loaded ? '✓' : '—' }}</td>
        </tr>
      </tbody>
    </table>

    <div class="actions">
      <button @click="refresh">↻ Refresh</button>
      <button @click="connectAll" :disabled="loading">Connect All</button>
    </div>
    <div v-if="msg" class="msg">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const devices = ref([])
const loading = ref(false)
const msg = ref('')

const connected = computed(() => devices.value.filter(d => d.connected).length)
const a2lLoaded = computed(() => devices.value.filter(d => d.a2l_loaded).length)

async function refresh() {
  try {
    const r = await invoke('list_devices')
    devices.value = r.devices
  } catch (e) { msg.value = String(e) }
}

async function connectAll() {
  loading.value = true
  msg.value = ''
  try {
    const r = await invoke('connect_all_devices')
    const ok = r.results.filter(x => !x.error).length
    msg.value = `Connected ${ok}/${r.results.length} devices`
    await refresh()
  } catch (e) { msg.value = String(e) }
  finally { loading.value = false }
}

onMounted(refresh)
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 16px; font-size: 1.4rem; }

.stats-row { display: flex; gap: 16px; margin-bottom: 24px; }
.stat-card { background: #0d0d0d; border: 1px solid #003311; padding: 12px 20px; border-radius: 4px; min-width: 100px; }
.stat-label { color: #006622; font-size: 0.75rem; text-transform: uppercase; margin-bottom: 4px; }
.stat-value { font-size: 1.8rem; color: #00cc44; }
.stat-value.connected { color: #00ff66; }

.dev-table { width: 100%; border-collapse: collapse; margin-bottom: 16px; }
.dev-table th { color: #006622; text-align: left; padding: 6px 10px; border-bottom: 1px solid #003311; font-size: 0.8rem; }
.dev-table td { padding: 8px 10px; border-bottom: 1px solid #001a09; }
.ok { color: #00ff66; }
.off { color: #444; }

.empty { color: #555; padding: 24px 0; }
.actions { display: flex; gap: 8px; margin-top: 8px; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 6px 14px; cursor: pointer; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.4; cursor: default; }
.msg { margin-top: 8px; color: #ffcc00; font-size: 0.85rem; }
</style>
