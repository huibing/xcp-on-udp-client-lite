<template>
  <div class="page">
    <h1>Compare Sessions</h1>

    <!-- Session Management -->
    <div class="sessions-bar">
      <select v-model="activeSession" @change="loadVariables" class="sel">
        <option value="">Select session…</option>
        <option v-for="s in sessions" :key="s.id" :value="s.id">{{ s.name }}</option>
      </select>
      <button @click="createSession">+ New Session</button>
      <button v-if="activeSession" @click="deleteSession" class="danger">Delete</button>
    </div>

    <div v-if="activeSession" class="session-body">
      <!-- Source loading -->
      <div class="sources-row">
        <div v-for="side in ['left', 'right']" :key="side" class="source-card">
          <h2>{{ side === 'left' ? 'Left' : 'Right' }} Source</h2>
          <div class="row">
            <select v-model="srcType[side]" class="sel-sm">
              <option value="a2l_hex">A2L + HEX</option>
              <option value="dcm">DCM</option>
            </select>
          </div>
          <div v-if="srcType[side] === 'a2l_hex'" class="src-inputs">
            <input v-model="srcA2L[side]" placeholder="A2L path" class="inp" />
            <input v-model="srcHex[side]" placeholder="HEX path" class="inp" />
          </div>
          <div v-else class="src-inputs">
            <input v-model="srcDcm[side]" placeholder="DCM path" class="inp" />
          </div>
          <div class="row">
            <button @click="loadSource(side)" :disabled="loadingSource === side">Load</button>
            <button @click="clearSource(side)" class="warn">Clear</button>
          </div>
          <div v-if="sourceInfo[side]" class="src-info">✓ {{ sourceInfo[side] }}</div>
        </div>
      </div>

      <!-- Variable filter + comparison -->
      <div class="var-toolbar">
        <input v-model="varSearch" @input="loadVariables" placeholder="Search variables…" class="inp" />
        <select v-model="presenceFilter" @change="loadVariables" class="sel-sm">
          <option value="all">All</option>
          <option value="both">Both sides</option>
          <option value="left_only">Left only</option>
          <option value="right_only">Right only</option>
          <option value="changed">Changed</option>
        </select>
        <button @click="loadVariables">↻ Refresh</button>
      </div>

      <div class="var-count">{{ variables.length }} variable(s)</div>

      <div class="var-table-wrap">
        <table class="var-table">
          <thead>
            <tr><th>Name</th><th>Left</th><th>Right</th><th>Match</th></tr>
          </thead>
          <tbody>
            <tr v-for="v in variables" :key="v.name" :class="v.match === false ? 'changed' : ''">
              <td>{{ v.name }}</td>
              <td class="mono">{{ v.left ?? '—' }}</td>
              <td class="mono">{{ v.right ?? '—' }}</td>
              <td>{{ v.match === true ? '✓' : v.match === false ? '≠' : '—' }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, reactive, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const sessions = ref([])
const activeSession = ref('')
const variables = ref([])
const varSearch = ref('')
const presenceFilter = ref('all')
const loadingSource = ref('')
const msg = ref('')
const msgErr = ref(false)

const srcType = reactive({ left: 'a2l_hex', right: 'a2l_hex' })
const srcA2L = reactive({ left: '', right: '' })
const srcHex = reactive({ left: '', right: '' })
const srcDcm = reactive({ left: '', right: '' })
const sourceInfo = reactive({ left: '', right: '' })

function setMsg(text, err = false) {
  msg.value = text; msgErr.value = err
  setTimeout(() => msg.value = '', 5000)
}

async function loadSessions() {
  try {
    const r = await invoke('compare_sessions')
    sessions.value = r
  } catch (e) { setMsg(String(e), true) }
}

async function createSession() {
  try {
    const s = await invoke('compare_create_session')
    await loadSessions()
    activeSession.value = s.id
    await loadVariables()
  } catch (e) { setMsg(String(e), true) }
}

async function deleteSession() {
  try {
    await invoke('compare_delete_session', { sessionId: activeSession.value })
    activeSession.value = ''
    variables.value = []
    await loadSessions()
  } catch (e) { setMsg(String(e), true) }
}

async function loadSource(side) {
  loadingSource.value = side
  const args = {
    sessionId: activeSession.value,
    side,
    sourceType: srcType[side],
    a2lPath: srcA2L[side] || null,
    hexPath: srcHex[side] || null,
    dcmPath: srcDcm[side] || null,
  }
  try {
    await invoke('compare_load_source', args)
    sourceInfo[side] = `Loaded (${srcType[side]})`
    await loadVariables()
    setMsg(`${side} source loaded`)
  } catch (e) { setMsg(String(e), true) }
  finally { loadingSource.value = '' }
}

async function clearSource(side) {
  try {
    await invoke('compare_clear_source', { sessionId: activeSession.value, side })
    sourceInfo[side] = ''
    await loadVariables()
  } catch (e) { setMsg(String(e), true) }
}

async function loadVariables() {
  if (!activeSession.value) return
  try {
    const r = await invoke('compare_variables', {
      sessionId: activeSession.value,
      search: varSearch.value || null,
      presence: presenceFilter.value,
      offset: 0,
      limit: 500,
    })
    variables.value = r.rows ?? r
  } catch (e) { setMsg(String(e), true) }
}

onMounted(loadSessions)
</script>

<style scoped>
.page { color: #00cc44; }
h1 { color: #00ff66; margin-bottom: 12px; font-size: 1.4rem; }
h2 { color: #00cc44; font-size: 0.95rem; margin-bottom: 8px; }

.sessions-bar { display: flex; gap: 8px; align-items: center; margin-bottom: 16px; }
.sel, .sel-sm { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; }
.sel-sm { font-size: 0.85rem; }
.inp { background: #0a0a0a; border: 1px solid #003311; color: #00cc44; padding: 5px 8px; font-family: inherit; width: 100%; box-sizing: border-box; margin-bottom: 4px; }
button { background: #001a09; border: 1px solid #00cc44; color: #00cc44; padding: 5px 10px; cursor: pointer; font-family: inherit; }
button:hover:not(:disabled) { background: #003311; }
button:disabled { opacity: 0.35; cursor: default; }
button.warn { border-color: #ffcc00; color: #ffcc00; }
button.danger { border-color: #ff4444; color: #ff4444; }

.sources-row { display: flex; gap: 12px; margin-bottom: 16px; }
.source-card { flex: 1; background: #0d0d0d; border: 1px solid #003311; padding: 12px; }
.src-inputs { margin: 6px 0; }
.row { display: flex; gap: 6px; }
.src-info { color: #00ff66; font-size: 0.8rem; margin-top: 6px; }

.var-toolbar { display: flex; gap: 8px; align-items: center; margin-bottom: 8px; flex-wrap: wrap; }
.var-count { color: #006622; font-size: 0.8rem; margin-bottom: 6px; }

.var-table-wrap { max-height: 400px; overflow-y: auto; }
.var-table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
.var-table th { color: #006622; text-align: left; padding: 4px 8px; border-bottom: 1px solid #003311; position: sticky; top: 0; background: #0a0a0a; }
.var-table td { padding: 4px 8px; border-bottom: 1px solid #001a09; }
.changed td { color: #ffcc00; }
.mono { font-family: monospace; }

.msg { margin-top: 8px; font-size: 0.85rem; color: #00cc44; }
.msg.err { color: #ff4444; }
</style>
