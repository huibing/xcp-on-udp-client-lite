<template>
  <div class="page">
    <h1>Compare</h1>

    <!-- Session tabs + new session button -->
    <div class="tabs-bar">
      <div class="tabs-scroll">
        <div v-for="s in sessions" :key="s.id"
          class="tab" :class="{ active: activeSessionId === s.id }"
          @click="selectSession(s.id)">
          <!-- Inline rename input (shown when editing this tab) -->
          <input v-if="editingTabId === s.id"
            v-model="editingTabName"
            class="tab-name-input"
            @keydown.enter.stop="commitTabRename"
            @keydown.escape.stop="cancelTabRename"
            @blur="commitTabRename"
            @click.stop
            ref="tabNameInput"
          />
          <span v-else
            class="tab-name"
            :title="activeSessionId === s.id ? 'Double-click to rename' : ''"
            @dblclick.stop="startTabRename(s)">{{ s.name }}</span>
          <span class="tab-count">{{ (s.selected_variables || []).length }}</span>
        </div>
      </div>
      <button @click="createSession" class="sm">+ New Session</button>
    </div>

    <!-- No sessions -->
    <div v-if="!activeSession" class="empty-state">
      No compare sessions. Create one to start.
    </div>

    <template v-else>
      <!-- Toolbar -->
      <div class="toolbar">
        <div class="toolbar-actions">
          <button class="sm" @click="openVarModal">Select Variables</button>
          <button class="sm secondary" @click="clearAllVariables" :disabled="!(activeSession.selected_variables || []).length">Clear All</button>
          <button class="sm danger" @click="deleteSession">Delete Session</button>
        </div>
        <span class="meta">{{ activeSession.name }} — {{ (activeSession.selected_variables || []).length }} variable(s) selected</span>
      </div>

      <!-- Left / Right source cards -->
      <div class="source-grid">
        <div v-for="side in ['left', 'right']" :key="side" class="source-card"
          :class="{ 'drag-over': draggingOver[side] }"
          :ref="el => { if (el) cardRefs[side] = el }">
          <h2>{{ side === 'left' ? 'Left Source' : 'Right Source' }}</h2>

          <!-- Loaded source summary -->
          <div class="source-summary">
            <template v-if="activeSession[side + '_source']">
              <div class="source-kind">{{ activeSession[side + '_source'].kind_label }}: <span class="bright">{{ activeSession[side + '_source'].label }}</span></div>
              <div class="source-detail">{{ activeSession[side + '_source'].detail }}</div>
              <div class="source-detail">{{ activeSession[side + '_source'].variable_count }} variable(s)</div>
            </template>
            <div v-else class="source-detail">No source loaded. Drop files here or use Browse.</div>
          </div>

          <!-- Source type selector -->
          <div class="field-row">
            <label class="field-label">Source Type</label>
            <select v-model="sourceType[side]" class="sel">
              <option value="a2l_hex">A2L + HEX</option>
              <option value="dcm">DCM</option>
            </select>
          </div>

          <!-- A2L + HEX inputs -->
          <template v-if="sourceType[side] === 'a2l_hex'">
            <div class="file-row">
              <span class="file-path">{{ sourceFiles[side].a2l || '(no A2L selected)' }}</span>
              <button class="sm" @click="browseFile(side, 'a2l')">Browse A2L…</button>
            </div>
            <div class="file-row">
              <span class="file-path">{{ sourceFiles[side].hex || '(no HEX selected)' }}</span>
              <button class="sm" @click="browseFile(side, 'hex')">Browse HEX…</button>
            </div>
          </template>

          <!-- DCM input -->
          <template v-else>
            <div class="file-row">
              <span class="file-path">{{ sourceFiles[side].dcm || '(no DCM selected)' }}</span>
              <button class="sm" @click="browseFile(side, 'dcm')">Browse DCM…</button>
            </div>
          </template>

          <!-- Load / Clear actions -->
          <div class="source-actions">
            <button @click="loadSource(side)" :disabled="loadingSource[side]">
              {{ loadingSource[side] ? 'Loading…' : 'Load' }}
            </button>
            <button class="sm secondary" @click="clearSource(side)">Clear</button>
          </div>
          <div v-if="loadStatus[side]" class="load-status" :class="loadStatusClass[side]">
            {{ loadStatus[side] }}
          </div>
        </div>
      </div>

      <!-- Comparison results -->
      <div v-if="rows.length === 0 && activeSession.left_source == null && activeSession.right_source == null" class="empty-state">
        Load a source on the left and right to compare values.
      </div>
      <div v-else-if="rows.length === 0" class="empty-state">
        No variables selected. Use "Select Variables" to choose compare targets.
      </div>
      <template v-else>
        <div class="results-toolbar">
          <span class="meta">{{ rows.length }} total — {{ diffCount }} diff{{ diffCount !== 1 ? 's' : '' }}</span>
          <div class="results-toolbar-actions">
            <label class="toggle-label">
              <input type="checkbox" v-model="showDiffOnly" @click="onClickShowDiffOnly"/>
              DIFF 
            </label>
            <label class="toggle-label">
              <input type="checkbox" @click="onClickShowCommon" v-model="showCommonOnly"/>
              COMMON
            </label>
            <button class="sm secondary" @click="toggleCollapseAll" :title=' allCollapsed ? "Expand All":"Collapsed ALL" '>{{ allCollapsed ? '▼' : '▲' }}</button>
          </div>
        </div>
        <div v-if="filteredRows.length === 0" class="empty-state">All selected variables match.</div>
        <div v-else class="results">
          <div v-for="row in filteredRows" :key="row.name"
            class="result-card" :class="{ diff: !row.matches, collapsed: collapsedRows.has(row.name) }">
            <div class="result-header" @click="toggleCollapse(row.name)">
              <div class="result-title">
                <strong>{{ row.name }}</strong>
              </div>
              <div class="result-badges">
                <span class="badge" :class="badgeClass(row)">{{ badgeLabel(row) }}</span>
                <span class="collapse-btn">▼</span>
              </div>
            </div>
            <div class="result-body">
              <div v-html="rowHtml(row)"></div>
            </div>
          </div>
        </div>
      </template>
    </template>

    <!-- Variable selection modal -->
    <div v-if="showVarModal" class="modal-overlay" @click.self="closeVarModal">
      <div class="modal">
        <div class="modal-header">
          <h2>Select Variables</h2>
        </div>
        <div class="modal-filters">
          <input v-model="varSearch" class="inp" placeholder="Search variable name…"
            @input="reloadVariables" />
          <select v-model="varPresence" class="sel" @change="reloadVariables">
            <option value="all">All Variables</option>
            <option value="common">Common</option>
            <option value="left_only">Left Only</option>
            <option value="right_only">Right Only</option>
          </select>
          <select v-model="varCalType" class="sel" @change="reloadVariables">
            <option value="">All Types</option>
            <option value="VALUE">VALUE</option>
            <option value="CURVE">CURVE</option>
            <option value="MAP">MAP</option>
            <option value="VAL_BLK">VAL_BLK</option>
            <option value="AXIS">AXIS</option>
            <option value="ASCII">ASCII</option>
          </select>
        </div>
        <div class="modal-body" @scroll="maybeLoadMore" ref="varModalBody">
          <table class="var-table">
            <thead>
              <tr>
                <th style="width:40px">✓</th>
                <th>Name</th>
                <th style="width:130px">Presence</th>
                <th style="width:160px">Left Type</th>
                <th style="width:160px">Right Type</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="item in varItems" :key="item.name"
                :class="{ selected: tempSelected.has(item.name) }"
                @click="toggleVarItem(item.name)">
                <td>{{ tempSelected.has(item.name) ? '✓' : '' }}</td>
                <td>{{ item.name }}</td>
                <td>{{ item.presence }}</td>
                <td>{{ item.left_type || '—' }}</td>
                <td>{{ item.right_type || '—' }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div class="modal-footer">
          <span class="meta">{{ tempSelected.size }} selected / {{ varPage.total }} matching</span>
          <div class="modal-footer-actions">
            <button class="sm secondary" @click="closeVarModal">Cancel</button>
            <button class="sm secondary" :disabled="selectingAll || varPage.total === 0" @click="selectAllFiltered">
              {{ selectingAll ? 'Selecting…' : 'Select All' }}
            </button>
            <button @click="applyVarSelection">Apply</button>
          </div>
        </div>
      </div>
    </div>

    <div v-if="msg" class="msg" :class="msgErr ? 'err' : ''">{{ msg }}</div>
  </div>
</template>

<script setup>
import { ref, computed, reactive, onMounted, onUnmounted, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { getCurrentWebview } from '@tauri-apps/api/webview'

// ─── Row HTML renderer ────────────────────────────────────────────────────────

function escHtml(s) {
  return String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

function diffSet(positions) {
  return new Set((positions || []).map(p => `${p.row}:${p.col}`))
}

function fmtAxis(v) {
  return v !== undefined ? Number(v).toFixed(3) : ''
}

function axisCell(leftVal, rightVal) {
  if (leftVal === undefined && rightVal === undefined) return '—'
  if (leftVal === undefined) return `<div class="merged-scalar"><div class="cell-left"><span class="stag">L</span>—</div><div class="cell-right"><span class="stag">R</span>${escHtml(fmtAxis(rightVal))}</div></div>`
  if (rightVal === undefined) return `<div class="merged-scalar"><div class="cell-left"><span class="stag">L</span>${escHtml(fmtAxis(leftVal))}</div><div class="cell-right"><span class="stag">R</span>—</div></div>`
  const ls = fmtAxis(leftVal), rs = fmtAxis(rightVal)
  if (ls === rs) return escHtml(ls)
  const isDiff = Math.abs(Number(leftVal) - Number(rightVal)) > 1e-6
  return `<div class="merged-scalar${isDiff ? ' diff-cell' : ''}"><div class="cell-left"><span class="stag">L</span>${escHtml(ls)}</div><div class="cell-right"><span class="stag">R</span>${escHtml(rs)}</div></div>`
}

function rowHtml(row) {
  const left = row.left?.value || null
  const right = row.right?.value || null
  const kind = (left || right)?.kind || 'scalar'
  const diffs = diffSet(row.diff_positions)
  const leftAx = row.left?.axis_values || []
  const rightAx = row.right?.axis_values || []

  if (kind === 'scalar') {
    const ld = left?.cells?.[0]?.[0]?.display ?? '—'
    const rd = right?.cells?.[0]?.[0]?.display ?? '—'
    const isDiff = diffs.has('0:0')
    return `<div class="merged-scalar${isDiff ? ' diff-cell' : ''}">` +
      `<div class="cell-left"><span class="stag">L</span>${escHtml(ld)}</div>` +
      `<div class="cell-right"><span class="stag">R</span>${escHtml(rd)}</div>` +
      `</div>`
  }

  const leftRows = left?.cells || []
  const rightRows = right?.cells || []
  const numRows = Math.max(leftRows.length, rightRows.length)
  if (numRows === 0) return '<div class="empty-state">No value data.</div>'
  const numCols = Math.max((leftRows[0] || rightRows[0] || []).length, 0)

  const leftX = leftAx[0] || [], rightX = rightAx[0] || []
  const leftY = leftAx[1] || [], rightY = rightAx[1] || []
  const hasXAxis = leftX.length > 0 || rightX.length > 0
  const hasYAxis = leftY.length > 0 || rightY.length > 0

  const colLabel = ci => hasXAxis ? axisCell(leftX[ci], rightX[ci]) : String(ci)
  const rowLabel = ri => hasYAxis ? axisCell(leftY[ri], rightY[ri]) : String(ri)

  const header = `<tr><th></th>${Array.from({ length: numCols }, (_, ci) =>
    `<th>${colLabel(ci)}</th>`).join('')}</tr>`

  const body = Array.from({ length: numRows }, (_, ri) => {
    const lr = leftRows[ri] || []
    const rr = rightRows[ri] || []
    const cells = Array.from({ length: numCols }, (_, ci) => {
      const isDiff = diffs.has(`${ri}:${ci}`)
      const ld = lr[ci]?.display ?? '—'
      const rd = rr[ci]?.display ?? '—'
      if (isDiff) {
        return `<td class="merged-cell diff-cell">` +
        `<div class="cell-left"><span class="stag">L</span>${escHtml(ld)}</div>` +
        `<div class="cell-right"><span class="stag">R</span>${escHtml(rd)}</div>` +
        `</td>`
      } else {
        return  `<td>` + `<div class="cell-left">${escHtml(ld)} </div>` + `</td>`
      }
      
    }).join('')
    return `<tr><th>${rowLabel(ri)}</th>${cells}</tr>`
  }).join('')

  return `<div class="grid-wrap"><table class="merged-grid"><thead>${header}</thead><tbody>${body}</tbody></table></div>`
}

// ─── State ────────────────────────────────────────────────────────────────────

const sessions = ref([])
const activeSessionId = ref(null)
const rows = ref([])
const showDiffOnly = ref(false)
const showCommonOnly = ref(false)
const collapsedRows = ref(new Set())

// Inline tab rename state
const editingTabId = ref(null)
const editingTabName = ref('')
const tabNameInput = ref(null)

const sourceType = reactive({ left: 'a2l_hex', right: 'a2l_hex' })
const sourceFiles = reactive({
  left: { a2l: '', hex: '', dcm: '' },
  right: { a2l: '', hex: '', dcm: '' }
})
const draggingOver = reactive({ left: false, right: false })
const cardRefs = reactive({ left: null, right: null })
const loadingSource = reactive({ left: false, right: false })
const loadStatus = reactive({ left: '', right: '' })
const loadStatusClass = reactive({ left: '', right: '' })

const showVarModal = ref(false)
const varSearch = ref('')
const varPresence = ref('all')
const varCalType = ref('')
const varItems = ref([])
const tempSelected = ref(new Set())
const varPage = reactive({ offset: 0, limit: 100, total: 0, loading: false, exhausted: false })
const varModalBody = ref(null)
const selectingAll = ref(false)

const msg = ref('')
const msgErr = ref(false)

// ─── Computed ─────────────────────────────────────────────────────────────────

const activeSession = computed(() =>
  sessions.value.find(s => s.id === activeSessionId.value) || null
)

const filteredRows = computed(() =>
  showDiffOnly.value ? rows.value.filter(r => !r.matches) : (showCommonOnly.value ? rows.value.filter(r => r.matches) : rows.value)
)

const diffCount = computed(() => rows.value.filter(r => !r.matches).length)

const allCollapsed = computed(() =>
  filteredRows.value.length > 0 && filteredRows.value.every(r => collapsedRows.value.has(r.name))
)

// ─── Helpers ──────────────────────────────────────────────────────────────────
function onClickShowDiffOnly() {
  if (showDiffOnly.value) {
    showDiffOnly.value = false
    showCommonOnly.value = true
  } else {
    showDiffOnly.value = true
    showCommonOnly.value = false
  }
}


function onClickShowCommon() {
  if (showCommonOnly.value) {
    showCommonOnly.value = false
    showDiffOnly.value = true
  } else {
    showCommonOnly.value = true
    showDiffOnly.value = false
  }
}

function setMsg(text, err = false) {
  msg.value = text
  msgErr.value = err
  setTimeout(() => { msg.value = '' }, 5000)
}

function setLoadStatus(side, cls, text) {
  loadStatus[side] = text
  loadStatusClass[side] = cls
  if (cls === 'success') {
    setTimeout(() => { if (loadStatus[side] === text) loadStatus[side] = '' }, 3000)
  }
}

function badgeClass(row) {
  if (row.matches) return 'badge-match'
  if (row.presence === 'common') return 'badge-diff'
  return 'badge-missing'
}

function badgeLabel(row) {
  if (row.matches) return 'MATCH'
  if (row.presence === 'common') return 'DIFF'
  return row.presence?.toUpperCase() || 'MISSING'
}

function toggleCollapse(name) {
  const s = new Set(collapsedRows.value)
  if (s.has(name)) s.delete(name)
  else s.add(name)
  collapsedRows.value = s
}

function toggleCollapseAll() {
  if (allCollapsed.value) {
    collapsedRows.value = new Set()
  } else {
    collapsedRows.value = new Set(filteredRows.value.map(r => r.name))
  }
}

// ─── Sessions ─────────────────────────────────────────────────────────────────

async function refreshSessions(preferredId) {
  try {
    const r = await invoke('compare_sessions')
    sessions.value = r.sessions ?? []
  } catch (e) {
    setMsg(String(e), true)
    return
  }
  if (!sessions.value.length) {
    activeSessionId.value = null
  } else if (preferredId && sessions.value.some(s => s.id === preferredId)) {
    activeSessionId.value = preferredId
  } else if (!sessions.value.some(s => s.id === activeSessionId.value)) {
    activeSessionId.value = sessions.value[0].id
  }
  await refreshResults()
}

async function selectSession(id) {
  // Cancel any in-progress rename before switching
  cancelTabRename()
  activeSessionId.value = id
  rows.value = []
  await refreshResults()
}

async function createSession() {
  try {
    const r = await invoke('compare_create_session', {})
    await refreshSessions(r.session?.id)
  } catch (e) { setMsg(String(e), true) }
}

function startTabRename(session) {
  editingTabId.value = session.id
  editingTabName.value = session.name
  nextTick(() => {
    if (tabNameInput.value) {
      const el = Array.isArray(tabNameInput.value) ? tabNameInput.value[0] : tabNameInput.value
      el?.focus()
      el?.select()
    }
  })
}

function cancelTabRename() {
  editingTabId.value = null
  editingTabName.value = ''
}

async function commitTabRename() {
  const id = editingTabId.value
  const name = editingTabName.value?.trim()
  editingTabId.value = null
  editingTabName.value = ''
  if (!id || !name) return
  try {
    await invoke('compare_rename_session', { sessionId: id, name })
    await refreshSessions(id)
  } catch (e) { setMsg(String(e), true) }
}

async function deleteSession() {
  if (!activeSession.value) return
  if (!window.confirm(`Delete session "${activeSession.value.name}"?`)) return
  try {
    await invoke('compare_delete_session', { sessionId: activeSession.value.id })
    await refreshSessions()
  } catch (e) { setMsg(String(e), true) }
}

// ─── Sources ──────────────────────────────────────────────────────────────────

async function browseFile(side, kind) {
  let title, filters
  if (kind === 'a2l') {
    title = 'Select A2L File'
    filters = [{ name: 'A2L Files', extensions: ['a2l', 'A2L'] }]
  } else if (kind === 'hex') {
    title = 'Select HEX File'
    filters = [{ name: 'HEX Files', extensions: ['hex', 's19', 'ihex', 'mot', 'srec', 'txt'] }]
  } else {
    title = 'Select DCM File'
    filters = [{ name: 'DCM Files', extensions: ['dcm', 'DCM'] }]
  }
  try {
    const path = await open({ multiple: false, directory: false, title, filters })
    if (path) sourceFiles[side][kind] = path
  } catch (e) { setMsg(String(e), true) }
}

const HEX_EXTS = new Set(['hex', 's19', 'ihex', 'mot', 'srec'])

// Tauri v2: use onDragDropEvent for real file paths. Position is in physical pixels;
// divide by devicePixelRatio to compare against CSS-pixel bounding rects.
function getSideFromPhysicalPos(px, py) {
  const dpr = window.devicePixelRatio || 1
  const x = px / dpr, y = py / dpr
  for (const [side, el] of Object.entries(cardRefs)) {
    if (!el) continue
    const r = el.getBoundingClientRect()
    if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return side
  }
  return null
}

async function processDroppedPaths(side, paths) {
  let changed = false
  for (const path of paths) {
    const ext = (path.split('.').pop() || '').toLowerCase()
    if (ext === 'a2l') {
      sourceFiles[side].a2l = path
      if (sourceType[side] === 'dcm') sourceType[side] = 'a2l_hex'
      changed = true
    } else if (HEX_EXTS.has(ext)) {
      sourceFiles[side].hex = path
      if (sourceType[side] === 'dcm') sourceType[side] = 'a2l_hex'
      changed = true
    } else if (ext === 'dcm') {
      sourceFiles[side].dcm = path
      if (sourceType[side] === 'a2l_hex') sourceType[side] = 'dcm'
      changed = true
    }
  }
  if (!changed) return
  const type = sourceType[side]
  if (type === 'dcm' && sourceFiles[side].dcm) {
    await loadSource(side)
  } else if (type === 'a2l_hex' && sourceFiles[side].a2l && sourceFiles[side].hex) {
    await loadSource(side)
  }
}

async function loadSource(side) {
  if (!activeSession.value) return
  const type = sourceType[side]
  if (type === 'a2l_hex') {
    if (!sourceFiles[side].a2l || !sourceFiles[side].hex) {
      setMsg('Please select both A2L and HEX files.', true)
      return
    }
  } else {
    if (!sourceFiles[side].dcm) {
      setMsg('Please select a DCM file.', true)
      return
    }
  }

  loadingSource[side] = true
  setLoadStatus(side, 'loading', 'Loading…')
  try {
    const args = {
      sessionId: activeSession.value.id,
      side,
      sourceType: type,
      filePath: type === 'a2l_hex' ? sourceFiles[side].a2l : sourceFiles[side].dcm,
    }
    if (type === 'a2l_hex') args.hexPath = sourceFiles[side].hex

    const r = await invoke('compare_load_source', args)
    if (r.success) {
      setLoadStatus(side, 'success', 'Source loaded successfully!')
      await refreshSessions(activeSession.value.id)
    } else {
      setLoadStatus(side, 'error', r.error || 'Failed to load source.')
    }
  } catch (e) {
    setLoadStatus(side, 'error', String(e))
  } finally {
    loadingSource[side] = false
  }
}

async function clearSource(side) {
  if (!activeSession.value) return
  try {
    await invoke('compare_clear_source', { sessionId: activeSession.value.id, side })
    await refreshSessions(activeSession.value.id)
  } catch (e) { setMsg(String(e), true) }
}

// ─── Results ──────────────────────────────────────────────────────────────────

async function refreshResults() {
  if (!activeSession.value) {
    rows.value = []
    return
  }
  try {
    const r = await invoke('compare_result', { sessionId: activeSession.value.id })
    rows.value = r.rows ?? []
    // Update session in list with fresh data (e.g. selected_variables count)
    if (r.session) {
      const idx = sessions.value.findIndex(s => s.id === r.session.id)
      if (idx >= 0) sessions.value[idx] = r.session
    }
  } catch (e) { setMsg(String(e), true) }
}

// ─── Variable modal ───────────────────────────────────────────────────────────

function openVarModal() {
  if (!activeSession.value) return
  tempSelected.value = new Set(activeSession.value.selected_variables || [])
  varSearch.value = ''
  varPresence.value = 'all'
  varCalType.value = ''
  varItems.value = []
  varPage.offset = 0
  varPage.total = 0
  varPage.exhausted = false
  showVarModal.value = true
  nextTick(() => loadVarPage(true))
}

function closeVarModal() {
  showVarModal.value = false
}

async function reloadVariables() {
  varPage.offset = 0
  varPage.exhausted = false
  varItems.value = []
  await loadVarPage(true)
}

async function loadVarPage(reset) {
  if (!activeSession.value || varPage.loading || varPage.exhausted) return
  varPage.loading = true
  const offset = reset ? 0 : varPage.offset
  try {
    const r = await invoke('compare_variables', {
      sessionId: activeSession.value.id,
      search: varSearch.value || null,
      presence: varPresence.value || null,
      calType: varCalType.value || null,
      offset,
      limit: varPage.limit,
    })
    if (!r.success) return
    const page = r.page || {}
    const items = page.items || []
    varPage.total = page.total || 0
    varPage.offset = offset + items.length
    varPage.exhausted = varPage.offset >= varPage.total
    if (reset) varItems.value = items
    else varItems.value = [...varItems.value, ...items]
  } catch (e) { setMsg(String(e), true) }
  finally { varPage.loading = false }
}

function maybeLoadMore(event) {
  const el = event.target
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 120) {
    loadVarPage(false)
  }
}

function toggleVarItem(name) {
  const s = new Set(tempSelected.value)
  if (s.has(name)) s.delete(name)
  else s.add(name)
  tempSelected.value = s
}

async function selectAllFiltered() {
  if (!activeSession.value || selectingAll.value) return
  selectingAll.value = true
  try {
    const r = await invoke('compare_variables', {
      sessionId: activeSession.value.id,
      search: varSearch.value || null,
      presence: varPresence.value || null,
      calType: varCalType.value || null,
      offset: 0,
      limit: 100000,
    })
    if (!r.success) return
    const items = r.page?.items || []
    const s = new Set(tempSelected.value)
    for (const item of items) s.add(item.name)
    tempSelected.value = s
  } catch (e) { setMsg(String(e), true) }
  finally { selectingAll.value = false }
}

async function applyVarSelection() {
  if (!activeSession.value) return
  try {
    await invoke('compare_select_variables', {
      sessionId: activeSession.value.id,
      names: Array.from(tempSelected.value),
    })
    closeVarModal()
    await refreshSessions(activeSession.value.id)
  } catch (e) { setMsg(String(e), true) }
}

async function clearAllVariables() {
  if (!activeSession.value) return
  try {
    await invoke('compare_select_variables', {
      sessionId: activeSession.value.id,
      names: [],
    })
    await refreshSessions(activeSession.value.id)
  } catch (e) { setMsg(String(e), true) }
}

// ─── Init ─────────────────────────────────────────────────────────────────────

let dndUnlisten = null

onMounted(async () => {
  refreshSessions()
  dndUnlisten = await getCurrentWebview().onDragDropEvent(event => {
    const p = event.payload
    if (p.type === 'enter' || p.type === 'over') {
      const side = getSideFromPhysicalPos(p.position.x, p.position.y)
      draggingOver.left = side === 'left'
      draggingOver.right = side === 'right'
    } else if (p.type === 'drop') {
      const side = getSideFromPhysicalPos(p.position.x, p.position.y)
      draggingOver.left = false
      draggingOver.right = false
      if (side && p.paths?.length) processDroppedPaths(side, p.paths)
    } else if (p.type === 'leave') {
      draggingOver.left = false
      draggingOver.right = false
    }
  })
})

onUnmounted(() => {
  if (dndUnlisten) dndUnlisten()
})
</script>

<style scoped>
/* ── Page ── */
.page { color: var(--exp-fg, #00cc44); }
h1 { color: var(--exp-fg-bright, #00ff66); margin-bottom: 14px; font-size: 1.4rem; }
h2 { color: var(--exp-fg, #00cc44); font-size: 1rem; margin-bottom: 10px; }

/* ── Tabs ── */
.tabs-bar { display: flex; align-items: center; gap: 8px; margin-bottom: 14px; flex-wrap: wrap; }
.tabs-scroll { display: flex; gap: 6px; overflow-x: auto; flex: 1; }
.tab {
  display: flex; align-items: center; gap: 6px;
  padding: 5px 12px; border: 1px solid var(--exp-border, #003311);
  background: var(--exp-bg, #0a0a0a); cursor: pointer; white-space: nowrap; font-size: 0.88rem;
}
.tab:hover { border-color: var(--exp-fg, #00cc44); }
.tab.active { border-color: var(--exp-fg-bright, #00ff66); color: var(--exp-fg-bright, #00ff66); background: var(--exp-border3, #001a09); }
.tab-count { color: var(--exp-fg-dim, #555); font-size: 0.75rem; }
.tab-name { cursor: default; }
.tab.active .tab-name { cursor: text; }
.tab-name-input {
  background: transparent; border: none; border-bottom: 1px solid var(--exp-fg-bright, #00ff66);
  color: var(--exp-fg-bright, #00ff66); font-family: Consolas, monospace; font-size: 0.88rem;
  padding: 0 2px; outline: none; min-width: 80px; max-width: 200px;
  width: max-content;
}

/* ── Toolbar ── */
.toolbar { display: flex; justify-content: space-between; align-items: center; gap: 8px; margin-bottom: 14px; flex-wrap: wrap; }
.toolbar-actions { display: flex; gap: 6px; flex-wrap: wrap; }
.meta { font-size: 0.8rem; color: var(--exp-fg-dim, #555); }

/* ── Source grid ── */
.source-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-bottom: 16px; }
@media (max-width: 760px) { .source-grid { grid-template-columns: 1fr; } }

.source-card { background: var(--exp-bg3, #0d0d0d); border: 1px solid var(--exp-border, #003311); padding: 14px; transition: border-color 0.15s, box-shadow 0.15s; }
.source-card.drag-over { border-color: var(--exp-fg-bright, #00ff66); background: var(--exp-border3, #001a0a); box-shadow: 0 0 14px rgba(0,255,102,0.25); }
.source-summary { background: var(--exp-bg2, #060606); border: 1px solid var(--exp-border3, #001a09); padding: 10px; margin-bottom: 12px; min-height: 60px; }
.source-kind { font-size: 0.88rem; }
.bright { color: var(--exp-fg-bright, #00ff66); font-weight: bold; }
.source-detail { color: var(--exp-fg-dim, #555); font-size: 0.8rem; margin-top: 3px; font-family: Consolas, monospace; }

.field-row { margin-bottom: 8px; }
.field-label { display: block; color: var(--exp-fg-dim3, #006622); font-size: 0.78rem; margin-bottom: 3px; }
.file-row { display: flex; align-items: center; gap: 6px; margin-bottom: 6px; flex-wrap: wrap; }
.file-path { font-size: 0.78rem; font-family: Consolas, monospace; color: var(--exp-fg-mid, #00aa33); flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.source-actions { display: flex; gap: 6px; margin-top: 10px; }
.load-status { margin-top: 6px; font-size: 0.82rem; }
.load-status.loading { color: #ffcc00; }
.load-status.success { color: var(--exp-fg-bright, #00ff66); }
.load-status.error { color: #ff4444; }

/* ── Result cards ── */
.results { display: flex; flex-direction: column; gap: 8px; }
.results-toolbar { display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px; }
.results-toolbar-actions { display: flex; align-items: center; gap: 10px; }
.toggle-label { display: flex; align-items: center; gap: 6px; cursor: pointer; font-size: 0.85rem; color: var(--exp-text-muted, #7a8a7a); }
.toggle-label input[type="checkbox"] { accent-color: var(--exp-accent, #00cc55); cursor: pointer; }

.result-card { background: var(--exp-bg3, #0d0d0d); border: 1px solid var(--exp-border, #003311); }
.result-card.diff { border-color: #ffcc00; }

.result-header {
  display: flex; justify-content: space-between; align-items: center;
  padding: 8px 12px; background: var(--exp-bg, #090909); cursor: pointer; user-select: none;
  border-bottom: 1px solid var(--exp-border, #003311);
}
.result-card.collapsed .result-header { border-bottom: none; }
.result-card.collapsed .result-body { display: none; }
.result-header:hover { background: var(--exp-bg5, #111); }

.result-title { font-size: 0.9rem; }
.result-badges { display: flex; align-items: center; gap: 8px; }
.badge { padding: 1px 8px; border-radius: 999px; font-size: 0.72rem; font-weight: 700; }
.badge-match { background: var(--exp-border3, #001a09); color: var(--exp-fg-bright, #00ff66); border: 1px solid var(--exp-border, #003311); }
.badge-diff { background: #1a0f00; color: #ffcc00; border: 1px solid #332200; }
.badge-missing { background: #1a1000; color: #ffcc00; border: 1px solid #332200; }

.collapse-btn { color: var(--exp-fg-dim, #555); font-size: 0.85rem; transition: transform 0.2s; }
.result-card.collapsed .collapse-btn { transform: rotate(-90deg); }

.result-body { padding: 12px; overflow-x: auto; }
.side-tag { color: var(--exp-fg, #00cc44); margin-right: 4px; font-weight: bold; }

/* ── Modal ── */
.modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.75); display: flex; align-items: center; justify-content: center; z-index: 100; }
.modal { background: var(--exp-bg3, #0d0d0d); border: 1px solid var(--exp-border, #003311); width: min(1000px, 92vw); max-height: 84vh; display: flex; flex-direction: column; }
.modal-header { padding: 14px 16px; border-bottom: 1px solid var(--exp-border, #003311); }
.modal-header h2 { margin: 0; }
.modal-filters { display: flex; gap: 8px; padding: 10px 16px; border-bottom: 1px solid var(--exp-border3, #001a09); }
.modal-body { overflow: auto; flex: 1; }
.modal-footer { display: flex; justify-content: space-between; align-items: center; padding: 10px 16px; border-top: 1px solid var(--exp-border, #003311); }
.modal-footer-actions { display: flex; gap: 8px; }

/* ── Variable table ── */
.var-table { width: 100%; border-collapse: collapse; font-size: 0.84rem; }
.var-table th { color: var(--exp-fg-dim3, #006622); text-align: left; padding: 5px 10px; border-bottom: 1px solid var(--exp-border, #003311); position: sticky; top: 0; background: var(--exp-bg3, #0d0d0d); }
.var-table td { padding: 5px 10px; border-bottom: 1px solid var(--exp-border3, #001a09); }
.var-table tbody tr { cursor: pointer; }
.var-table tbody tr:hover { background: var(--exp-hover, #0a1a0a); }
.var-table tbody tr.selected { background: var(--exp-border3, #001a09); color: var(--exp-fg-bright, #00ff66); }

/* ── Shared form elements ── */
button {
  background: var(--exp-border3, #001a09); border: 1px solid var(--exp-fg, #00cc44); color: var(--exp-fg, #00cc44);
  padding: 5px 12px; cursor: pointer; font-family: Consolas, monospace; font-size: 0.88rem;
}
button:hover:not(:disabled) { background: var(--exp-hover, #003311); }
button:disabled { opacity: 0.35; cursor: default; }
button.sm { padding: 3px 8px; font-size: 0.8rem; }
button.secondary { border-color: var(--exp-fg-dim, #555); color: var(--exp-fg-dim, #777); }
button.secondary:hover:not(:disabled) { background: var(--exp-bg5, #111); border-color: var(--exp-fg-dim, #777); color: var(--exp-fg-dim, #aaa); }
button.danger { border-color: #ff4444; color: #ff4444; }
button.danger:hover:not(:disabled) { background: #1a0000; }

.inp { background: var(--exp-bg4, #0a0a0a); border: 1px solid var(--exp-border, #003311); color: var(--exp-fg, #00cc44); padding: 5px 8px; font-family: Consolas, monospace; font-size: 0.88rem; }
.sel { background: var(--exp-bg4, #0a0a0a); border: 1px solid var(--exp-border, #003311); color: var(--exp-fg, #00cc44); padding: 5px 8px; font-family: Consolas, monospace; font-size: 0.88rem; }
.inp:focus, .sel:focus { outline: 1px solid var(--exp-fg, #00cc44); }

.empty-state { color: var(--exp-fg-dim, #555); padding: 28px 0; text-align: center; border: 1px dashed var(--exp-border3, #001a09); margin-bottom: 16px; }
.msg { margin-top: 8px; font-size: 0.85rem; color: var(--exp-fg, #00cc44); }
.msg.err { color: #ff4444; }

/* ── Merged value grid (injected via v-html) ── */
:deep(.merged-scalar) {
  display: inline-flex; flex-direction: column; gap: 3px;
  font-family: Consolas, monospace; font-size: 0.95rem;
  padding: 8px 12px; background: var(--exp-bg2, #060606); min-width: 5rem;
}
:deep(.merged-scalar.diff-cell) { background: #1a1000; }

:deep(.cell-left) { color: var(--exp-fg, #00cc44); font-size: 0.85rem; }
:deep(.cell-right) { color: var(--exp-fg-mid, #00aa33); font-size: 0.85rem; border-top: 1px solid var(--exp-border, #003311); margin-top: 2px; padding-top: 2px; }
:deep(.stag) { font-size: 0.7em; font-weight: 700; color: var(--exp-fg-dim, #555); margin-right: 4px; }

:deep(.grid-wrap) { overflow-x: auto; max-width: 100%; margin-top: 4px; }
:deep(.merged-grid) { border-collapse: collapse; white-space: nowrap; }
:deep(.merged-grid th),
:deep(.merged-grid td) { border: 1px solid var(--exp-border2, #002211); padding: 3px 8px; text-align: center; font-family: Consolas, monospace; font-size: 0.8rem; }
:deep(.merged-grid th) { color: var(--exp-fg-dim, #555); background: var(--exp-bg, #0a0a0a); }
:deep(.merged-grid th .merged-scalar) { font-size: 0.7rem; padding: 2px 4px; min-width: auto; background: none; }
:deep(.merged-grid th .merged-scalar .cell-left),
:deep(.merged-grid th .merged-scalar .cell-right) { font-size: 0.7rem; }
:deep(.merged-cell) { vertical-align: top; min-width: 60px; }
:deep(.diff-cell) { background: #1a1000 !important; }
:deep(.diff-cell .cell-left),
:deep(.diff-cell .cell-right) { color: #ffcc00; font-weight: bold; }
</style>
