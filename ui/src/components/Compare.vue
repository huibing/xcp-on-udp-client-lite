<template>
  <div class="page">
    <h1>Compare Sessions</h1>

    <div class="toolbar">
      <button @click="showCapture = true">+ Capture Session</button>
      <button @click="loadSessions">↻ Refresh</button>
      <span class="count-info">{{ sessions.length }} session(s)</span>
    </div>

    <div class="card" v-if="sessions.length > 0">
      <h2>Sessions</h2>
      <table class="sess-table">
        <thead>
          <tr><th>Name</th><th>Timestamp</th><th>Devices</th><th>Actions</th></tr>
        </thead>
        <tbody>
          <tr v-for="s in sessions" :key="s.id"
            :class="{ 'sel-a': sessionA === s.id, 'sel-b': sessionB === s.id }">
            <td class="sess-name">{{ s.name }}</td>
            <td class="mono">{{ formatTs(s.timestamp) }}</td>
            <td>{{ s.device_count }}</td>
            <td class="actions-cell">
              <button @click="sessionA = s.id" :class="{ active: sessionA === s.id }" class="sm">A</button>
              <button @click="sessionB = s.id" :class="{ active: sessionB === s.id }" class="sm">B</button>
              <button @click="exportSession(s.id)" class="sm">⬇ Export</button>
              <button @click="deleteSession(s.id)" class="sm danger">✕</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-else class="empty">No sessions yet. Capture a session to start.</div>

    <div class="card" v-if="sessions.length >= 2">
      <h2>Compare</h2>
      <div class="compare-bar">
        <div class="sel-indicator a">A: {{ sessionName(sessionA) || '(none)' }}</div>
        <span style="color:#555">vs</span>
        <div class="sel-indicator b">B: {{ sessionName(sessionB) || '(none)' }}</div>
        <button @click="runCompare"
          :disabled="!sessionA || !sessionB || sessionA === sessionB || comparing">
          {{ comparing ? 'Comparing…' : '⚙ Compare' }}
        </button>
      </div>

      <template v-if="compared">
        <div class="diff-toolbar">
          <input v-model="diffSearch" placeholder="Search params…" class="inp" />
          <button @click="showDiffsOnly = !showDiffsOnly" :class="{ active: showDiffsOnly }">
            {{ showDiffsOnly ? '≠ Diffs Only' : '≈ All' }}
          </button>
          <span class="count-info">
            <span class="green-txt">✓ {{ matchCount }}</span> /
            <span class="red-txt">≠ {{ mismatchCount }}</span>
          </span>
        </div>

        <div class="diff-table-wrap">
          <table class="diff-table">
            <thead>
              <tr><th>Parameter</th><th>Device</th><th class="va">Value A</th><th class="vb">Value B</th><th>Match</th></tr>
            </thead>
            <tbody>
              <tr v-for="d in filteredDiffs" :key="d.name + d.device"
                :class="d.match ? 'match-row' : 'diff-row'">
                <td>{{ d.name }}</td>
                <td class="dev-col">{{ d.device }}</td>
                <td class="va mono">{{ d.value_a }}</td>
                <td class="vb mono">{{ d.value_b }}</td>
                <td class="match-col">{{ d.match ? '✓' : '≠' }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </template>
    </div>

    <!-- Capture modal -->
    <div v-if="showCapture" class="modal-overlay" @click.self="showCapture = false">
      <div class="modal">
        <h2>Capture Session</h2>
        <label class="field-label">Session Name</label>
        <input v-model="captureName" class="inp full" placeholder="e.g. Baseline 2025-01-01" />
        <label class="field-label" style="margin-top:12px">Select Devices</label>
        <div class="device-check-list">
          <label v-for="d in allDevices" :key="d.id" class="dev-check"
            :class="{ disabled: !d.connected }">
            <input type="checkbox" :value="d.id" v-model="captureDevices" :disabled="!d.connected" />
            <span class="dcn">{{ d.name }}</span>
            <span class="dca">{{ d.address }}</span>
            <span v-if="!d.connected" class="offline-tag">offline</span>
          </label>
        </div>
        <div class="modal-actions">
          <button @click="captureSession"
            :disabled="!captureName || captureDevices.length === 0 || capturing">
            {{ capturing ? 'Capturing…' : '📸 Capture' }}
          </button>
          <button @click="showCapture = false" class="warn">Cancel</button>
        </div>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const sessions = ref([])
const allDevices = ref([])
const sessionA = ref('')
const sessionB = ref('')
const diffs = ref([])
const compared = ref(false)
const diffSearch = ref('')
const showDiffsOnly = ref(false)
const comparing = ref(false)
const showCapture = ref(false)
const captureName = ref('')
const captureDevices = ref([])
const capturing = ref(false)
const msg = ref('')
const msgErr = ref(false)

const matchCount = computed(() => diffs.value.filter(d => d.match).length)
const mismatchCount = computed(() => diffs.value.filter(d => !d.match).length)

const filteredDiffs = computed(() => {
  let list = diffs.value
  if (showDiffsOnly.value) list = list.filter(d => !d.match)
  if (diffSearch.value) {
    const q = diffSearch.value.toLowerCase()
    list = list.filter(d =>
      d.name.toLowerCase().includes(q) || (d.device || '').toLowerCase().includes(q))
  }
  return list
})

function sessionName(id) {
  return sessions.value.find(s => s.id === id)?.name || ''
}

function formatTs(ts) {
  if (!ts) return '—'
  try { return new Date(ts).toLocaleString() } catch { return String(ts) }
}

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 5000)
}

async function loadSessions() {
  try {
    const r = await invoke('compare_list_sessions')
    sessions.value = r.sessions ?? []
  } catch (e) { setMsg(String(e), true) }
}

async function loadDevices() {
  try {
    const r = await invoke('list_devices')
    allDevices.value = r.devices ?? []
  } catch { }
}

async function captureSession() {
  if (!captureName.value || captureDevices.value.length === 0) return
  capturing.value = true
  try {
    await invoke('compare_capture_session', {
      name: captureName.value,
      deviceIds: captureDevices.value
    })
    setMsg(`Session captured: ${captureName.value}`)
    showCapture.value = false
    captureName.value = ''
    captureDevices.value = []
    await loadSessions()
  } catch (e) { setMsg(String(e), true) }
  finally { capturing.value = false }
}

async function deleteSession(id) {
  try {
    await invoke('compare_delete_session', { sessionId: id })
    if (sessionA.value === id) sessionA.value = ''
    if (sessionB.value === id) sessionB.value = ''
    diffs.value = []; compared.value = false
    await loadSessions()
  } catch (e) { setMsg(String(e), true) }
}

async function runCompare() {
  if (!sessionA.value || !sessionB.value) return
  comparing.value = true; diffs.value = []; compared.value = false
  try {
    const r = await invoke('compare_sessions', {
      sessionA: sessionA.value,
      sessionB: sessionB.value
    })
    diffs.value = r.diffs ?? []
    compared.value = true
  } catch (e) { setMsg(String(e), true) }
  finally { comparing.value = false }
}

async function exportSession(id) {
  try {
    const r = await invoke('compare_export', { sessionId: id })
    const blob = new Blob([r.csv], { type: 'text/csv' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    const name = sessionName(id) || id
    a.href = url; a.download = `compare_${name}.csv`; a.click()
    URL.revokeObjectURL(url)
  } catch (e) { setMsg(String(e), true) }
}

onMounted(async () => { await loadSessions(); await loadDevices() })
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 14px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 1rem; margin-bottom: 10px; }
.toolbar { display: flex; gap: 8px; align-items: center; margin-bottom: 16px; flex-wrap: wrap; }
.count-info { font-size: 0.8rem; color: #555; }
.green-txt { color: #00ff66; }
.red-txt { color: #ff4444; }
.card { background: #0d0d0d; border: 1px solid #003311; padding: 14px; margin-bottom: 16px; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: Consolas, monospace; font-size: 0.9rem; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
button.sm { padding: 2px 8px; font-size: 0.8rem; }
button.active { background: #003311; color: #00ff66; }
button.warn { border-color: #ffcc00; color: #ffcc00; }
button.warn:hover:not(:disabled) { background: #1a1100; }
button.danger { border-color: #ff4444; color: #ff4444; }
button.danger:hover:not(:disabled) { background: #1a0000; }
.sess-table { width: 100%; border-collapse: collapse; font-size: 0.88rem; }
.sess-table th { color: #006622; text-align: left; padding: 5px 8px; border-bottom: 1px solid #003311; }
.sess-table td { padding: 5px 8px; border-bottom: 1px solid #001a09; }
.sess-table tr.sel-a td { background: rgba(0,80,255,0.05); }
.sess-table tr.sel-b td { background: rgba(255,120,0,0.05); }
.sess-name { color: #00ff66; font-weight: bold; }
.actions-cell { display: flex; gap: 4px; }
.mono { font-family: Consolas, monospace; }
.empty { color: #555; padding: 24px 0; }
.compare-bar { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
.sel-indicator { padding: 4px 10px; border: 1px solid; font-size: 0.85rem; }
.sel-indicator.a { color: #5599ff; border-color: #224; }
.sel-indicator.b { color: #ff9933; border-color: #420; }
.diff-toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 10px; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: Consolas, monospace; }
.inp.full { width: 100%; box-sizing: border-box; }
.diff-table-wrap { max-height: 400px; overflow-y: auto; }
.diff-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
.diff-table th { color: #006622; text-align: left; padding: 5px 8px; border-bottom: 1px solid #003311; position: sticky; top: 0; background: #0d0d0d; }
.diff-table td { padding: 4px 8px; border-bottom: 1px solid #001a09; }
.diff-table tr.diff-row { background: #1a0800; }
.diff-table tr.diff-row td { color: #ffcc00; }
.va { color: #5599ff; }
.vb { color: #ff9933; }
.match-col { text-align: center; font-weight: bold; }
.modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.7); display: flex; align-items: center; justify-content: center; z-index: 100; }
.modal { background: #0d0d0d; border: 1px solid #003311; padding: 20px; min-width: 460px; max-height: 80vh; overflow-y: auto; }
.field-label { display: block; color: #006622; font-size: 0.8rem; margin-bottom: 4px; }
.device-check-list { max-height: 200px; overflow-y: auto; border: 1px solid #003311; padding: 8px; margin-bottom: 12px; }
.dev-check { display: flex; align-items: center; gap: 8px; padding: 4px 0; cursor: pointer; }
.dev-check.disabled { opacity: 0.4; cursor: not-allowed; }
.dev-check input { accent-color: #00cc44; }
.dcn { color: #00ff66; font-weight: bold; font-size: 0.9rem; }
.dca { color: #555; font-size: 0.8rem; font-family: Consolas, monospace; }
.offline-tag { color: #ff4444; font-size: 0.75rem; }
.modal-actions { display: flex; gap: 8px; }
.msg { margin-top: 8px; font-size: 0.85rem; color: #00cc44; }
.msg.err { color: #ff4444; }
</style>
