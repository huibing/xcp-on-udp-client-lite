<template>
  <Teleport to="#xcp-app">
    <div class="derived-overlay" @click.self="$emit('close')">
      <div class="derived-dialog">

        <!-- Header -->
        <div class="derived-header">
          <span class="derived-title">{{ initialName ? 'ƒ(x) Edit Derived Signal' : 'ƒ(x) New Derived Signal' }}</span>
          <button class="derived-close-btn" @click="$emit('close')" title="Close">✕</button>
        </div>

        <!-- Signal name -->
        <div class="derived-section">
          <label class="derived-label">Signal Name</label>
          <input
            v-model.trim="signalName"
            class="derived-input"
            placeholder="e.g. speed_rad  or  throttle_filtered"
            @input="clearPreview"
          />
        </div>

        <!-- Variable bindings -->
        <div class="derived-section">
          <div class="derived-label-row">
            <label class="derived-label">Input Variables</label>
            <button class="sm" @click="addVar">+ Add</button>
          </div>
          <p class="derived-hint">
            Assign a short name to each source signal you want to use in the formula.
          </p>

          <div v-if="vars.length === 0" class="derived-empty">
            No variables yet — click <strong>+ Add</strong> to bind a source signal.
          </div>

          <div v-for="(v, i) in vars" :key="i" class="derived-var-section">
            <div class="derived-var-row">
              <input
                v-model.trim="v.varName"
                class="derived-var-name"
                placeholder="a"
                title="Variable name used in the formula"
                :class="{ 'inp-error': v.varName && !isValidIdent(v.varName) }"
                @input="clearPreview"
              />
              <span class="derived-eq">=</span>
              <select
                v-model="v.transform"
                class="derived-transform-select"
                title="Optional transformation applied before the formula"
                @change="clearPreview"
              >
                <option value="none">raw</option>
                <option value="diff">d/dt</option>
                <option value="integ">∫ dt</option>
              </select>
              <div class="derived-sig-picker">
                <input
                  v-model="v.searchText"
                  class="derived-sig-search"
                  :class="{ 'has-selection': v.signalKey && v.searchText === v.sigLabel }"
                  placeholder="search channel…   (* wildcard)"
                  @focus="v.dropdownOpen = true"
                  @input="onSigSearch(v)"
                  @blur="scheduleCloseDropdown(v)"
                  @keydown.escape="v.dropdownOpen = false"
                />
                <div v-if="v.dropdownOpen" class="derived-sig-dropdown">
                  <template v-if="filteredSignals(v.searchText).length > 0">
                    <div
                      v-for="sig in filteredSignals(v.searchText)"
                      :key="sig.key"
                      class="derived-sig-item"
                      :class="{ selected: v.signalKey === sig.key }"
                      @mousedown.prevent="selectSignal(v, sig)"
                    >
                      <span class="derived-sig-channel">{{ sig.channelName }}</span>
                      <span class="derived-sig-file">{{ sig.label.split('  [')[1]?.slice(0, -1) }}</span>
                    </div>
                  </template>
                  <div v-else class="derived-sig-no-results">No matching channels</div>
                </div>
              </div>
              <button class="derived-icon-btn" @click="removeVar(i)" title="Remove">✕</button>
            </div>
          </div>
        </div>

        <!-- Formula -->
        <div class="derived-section">
          <label class="derived-label">Formula</label>
          <textarea
            v-model="expression"
            class="derived-expr"
            rows="3"
            placeholder="e.g.  sin(a) * 2 + b    or    a > 0 && b < 100"
            spellcheck="false"
            @input="clearPreview"
          />
          <details class="derived-ref">
            <summary class="derived-ref-toggle">Reference — supported operations</summary>
            <div class="derived-ref-body">
              <div class="derived-ref-row">
                <strong>Arithmetic</strong>
                <code>+ &nbsp; − &nbsp; * &nbsp; / &nbsp; % &nbsp; ^ (power)</code>
              </div>
              <div class="derived-ref-row">
                <strong>Trig</strong>
                <code>sin &nbsp; cos &nbsp; tan &nbsp; asin &nbsp; acos &nbsp; atan &nbsp; atan2 &nbsp; sinh &nbsp; cosh &nbsp; tanh</code>
              </div>
              <div class="derived-ref-row">
                <strong>Math</strong>
                <code>sqrt &nbsp; abs &nbsp; log &nbsp; log2 &nbsp; log10 &nbsp; exp &nbsp; pow &nbsp; ceil &nbsp; floor &nbsp; round &nbsp; sign &nbsp; hypot &nbsp; min &nbsp; max &nbsp; clamp(x,lo,hi)</code>
              </div>
              <div class="derived-ref-row">
                <strong>Comparison</strong>
                <code>== &nbsp; != &nbsp; &lt; &nbsp; &lt;= &nbsp; &gt; &nbsp; &gt;=</code>
                <span class="derived-ref-note">(return 0 or 1)</span>
              </div>
              <div class="derived-ref-row">
                <strong>Logical</strong>
                <code>&amp;&amp; &nbsp; || &nbsp; !</code>
                <span class="derived-ref-note">(short-circuit, return 0 or 1)</span>
              </div>
              <div class="derived-ref-row">
                <strong>Ternary</strong>
                <code>cond ? valueIfTrue : valueIfFalse</code>
                <span class="derived-ref-note">(short-circuit)</span>
              </div>
              <div class="derived-ref-row">
                <strong>Constants</strong>
                <code>pi &nbsp; e</code>
              </div>
              <div class="derived-ref-row">
                <strong>Filters</strong>
                <code>Lowpass(expr, fc) &nbsp; Highpass(expr, fc) &nbsp; Bandpass(expr, loHz, hiHz)</code>
                <span class="derived-ref-note">fc/loHz/hiHz = cutoff frequency in Hz; wraps any sub-expression, nestable</span>
              </div>
            </div>
          </details>
        </div>

        <!-- Status / preview -->
        <div v-if="previewError" class="derived-status derived-err">⚠ {{ previewError }}</div>
        <div v-else-if="previewOk" class="derived-status derived-ok">
          ✓ Expression valid — {{ previewSamples.toLocaleString() }} samples computed
        </div>

        <!-- Actions -->
        <div class="derived-actions">
          <button class="secondary" @click="$emit('close')">Cancel</button>
          <button
            class="secondary"
            :disabled="!canValidate || validating"
            @click="validate"
          >{{ validating ? 'Loading…' : 'Validate' }}</button>
          <button
            :disabled="!previewOk || !signalName"
            @click="confirm"
          >{{ initialName ? 'Update Signal' : 'Add to Chart' }}</button>
        </div>

      </div>
    </div>
  </Teleport>
</template>

<script setup>
import { ref, computed } from 'vue'
import { compile, computeDerivedSignal, differentiate, integrate } from '../utils/expressionEvaluator.js'

// ── Props / Emits ─────────────────────────────────────────────────────────────

const props = defineProps({
  /** Array of { key, label, filePath, channelName } — all channels from all opened files */
  availableSignals: {
    type: Array,
    required: true,
  },
  /** Map<key, { times: Float64Array, values: Float64Array }> */
  historyStore: {
    type: Map,
    required: true,
  },
  /** async (filePath, channelName) => { times, values } — loads a channel on demand */
  loadSignalData: {
    type: Function,
    required: true,
  },
  // ── Edit-mode: pre-populate the form with an existing derived signal ──────
  initialName: {
    type: String,
    default: '',
  },
  initialExpression: {
    type: String,
    default: '',
  },
  /** Array of { var_name, signal_key } */
  initialVars: {
    type: Array,
    default: () => [],
  },
})

const emit = defineEmits(['close', 'add'])

// ── Wildcard search ───────────────────────────────────────────────────────────

function wildcardToRegex(pattern) {
  const escaped = pattern.replace(/[.+^${}()|[\]\\]/g, '\\$&')
  return new RegExp(escaped.replace(/\*/g, '.*'), 'i')
}

const MAX_RESULTS = 100

function filteredSignals(searchText) {
  if (!searchText || !searchText.trim()) {
    return props.availableSignals.slice(0, MAX_RESULTS)
  }
  const re = wildcardToRegex(searchText.trim())
  return props.availableSignals.filter(s => re.test(s.label) || re.test(s.channelName)).slice(0, MAX_RESULTS)
}

// ── Local state ───────────────────────────────────────────────────────────────

const signalName = ref(props.initialName)
const expression = ref(props.initialExpression)

function makeVar(v) {
  // Resolve label + filePath/channelName from availableSignals for edit-mode pre-population
  const sig = props.availableSignals.find(s => s.key === v.signal_key) ?? null
  return {
    varName:      v.var_name,
    signalKey:    v.signal_key,
    filePath:     sig?.filePath     ?? '',
    channelName:  sig?.channelName  ?? '',
    sigLabel:     sig?.label        ?? v.signal_key,
    searchText:   sig?.label        ?? v.signal_key,
    dropdownOpen: false,
    transform:    v.transform       ?? 'none',
  }
}

const vars = ref(props.initialVars.map(makeVar))

const previewError   = ref(null)
const previewOk      = ref(false)
const previewSamples = ref(0)
const validating     = ref(false)
let   previewResult  = null   // { times, values } from last successful compute

// ── Helpers ───────────────────────────────────────────────────────────────────

function isValidIdent(name) {
  return /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(name)
}

function clearPreview() {
  previewError.value   = null
  previewOk.value      = false
  previewSamples.value = 0
  previewResult        = null
}

function addVar() {
  vars.value.push({ varName: '', signalKey: '', filePath: '', channelName: '', sigLabel: '', searchText: '', dropdownOpen: false, transform: 'none' })
  clearPreview()
}

function removeVar(i) {
  vars.value.splice(i, 1)
  clearPreview()
}

// ── Signal picker helpers ─────────────────────────────────────────────────────

function onSigSearch(v) {
  v.signalKey   = ''
  v.filePath    = ''
  v.channelName = ''
  v.sigLabel    = ''
  v.dropdownOpen = true
  clearPreview()
}

function selectSignal(v, sig) {
  v.signalKey   = sig.key
  v.filePath    = sig.filePath
  v.channelName = sig.channelName
  v.sigLabel    = sig.label
  v.searchText  = sig.label
  v.dropdownOpen = false
  clearPreview()
}

function scheduleCloseDropdown(v) {
  setTimeout(() => { v.dropdownOpen = false }, 150)
}

// Is validation possible right now?
const canValidate = computed(() => {
  if (!expression.value.trim()) return false
  if (validating.value) return false
  for (const v of vars.value) {
    if (!v.varName || !v.signalKey) return false
    if (!isValidIdent(v.varName))   return false
  }
  return true
})

// ── Validation / Preview ──────────────────────────────────────────────────────

async function validate() {
  clearPreview()
  validating.value = true

  try {
    // 1. Check for duplicate variable names
    const names = vars.value.map(v => v.varName)
    const dupName = names.find((n, i) => names.indexOf(n) !== i)
    if (dupName) {
      previewError.value = `Duplicate variable name '${dupName}'`
      return
    }

    // 2. Load signal data (from historyStore cache or on-demand from file)
    const signalData = {}
    for (const v of vars.value) {
      let data = props.historyStore.get(v.signalKey)
      if (!data) {
        try {
          data = await props.loadSignalData(v.filePath, v.channelName)
        } catch (err) {
          previewError.value = `Failed to load '${v.sigLabel}': ${err}`
          return
        }
      }
      // Apply per-variable transform (d/dt or ∫)
      if (v.transform === 'diff') {
        data = { times: data.times, values: differentiate(data.times, data.values) }
      } else if (v.transform === 'integ') {
        data = { times: data.times, values: integrate(data.times, data.values) }
      }
      signalData[v.varName] = data
    }

    // 3. Check that the expression uses only known variables
    try {
      const { variables } = compile(expression.value)
      for (const usedVar of variables) {
        if (!(usedVar in signalData)) {
          previewError.value = `Formula uses variable '${usedVar}' which is not defined above`
          return
        }
      }
    } catch (err) {
      previewError.value = `Syntax error: ${err.message}`
      return
    }

    // 4. Compute the derived signal
    try {
      const result = computeDerivedSignal(expression.value, signalData)
      if (result.times.length === 0) {
        previewError.value = 'Result is empty — source signals have no data'
        return
      }
      previewResult        = result
      previewSamples.value = result.times.length
      previewOk.value      = true
    } catch (err) {
      previewError.value = `Compute error: ${err.message}`
    }
  } finally {
    validating.value = false
  }
}

// ── Confirm (emit result) ─────────────────────────────────────────────────────

async function confirm() {
  if (!previewOk.value) await validate()
  if (!previewOk.value || !previewResult || !signalName.value) return
  emit('add', {
    name:        signalName.value,
    expression:  expression.value,
    derivedVars: vars.value.map(v => ({
      var_name:   v.varName,
      signal_key: v.signalKey,
      transform:  v.transform !== 'none' ? v.transform : undefined,
    })),
    times:       previewResult.times,
    values:      previewResult.values,
  })
}
</script>

<style scoped>
/* ── Overlay & dialog shell ─────────────────────────────────────────────────── */
.derived-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.72);
  z-index: 2000;
  display: flex;
  align-items: center;
  justify-content: center;
}

.derived-dialog {
  background: var(--exp-bg3);
  border: 1px solid var(--exp-border);
  border-radius: 8px;
  padding: 24px;
  width: min(680px, 94vw);
  max-height: 92vh;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 18px;
  box-shadow: 0 8px 40px rgba(0, 0, 0, 0.7);
  font-family: var(--exp-font, 'Consolas', 'Courier New', monospace);
  font-size: var(--exp-font-size, 13px);
  color: var(--exp-fg);
}

/* ── Header ─────────────────────────────────────────────────────────────────── */
.derived-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.derived-title {
  font-size: 1.1rem;
  font-weight: 700;
  color: var(--exp-fg-bright);
}

.derived-close-btn {
  background: transparent;
  border: 1px solid var(--exp-border);
  color: var(--exp-fg-dim);
  cursor: pointer;
  font-size: 0.9rem;
  padding: 2px 8px;
  border-radius: 3px;
  font-family: inherit;
}
.derived-close-btn:hover {
  color: #ff6666;
  border-color: #ff4444;
  background: rgba(255, 100, 100, 0.1);
}

/* ── Section layout ─────────────────────────────────────────────────────────── */
.derived-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.derived-label {
  font-size: 0.82rem;
  font-weight: 700;
  color: var(--exp-fg-mid);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.derived-label-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.derived-hint {
  font-size: 0.78rem;
  color: var(--exp-fg-dim3);
  margin: 0;
}

/* ── Buttons ─────────────────────────────────────────────────────────────────── */
button {
  background: var(--exp-border3);
  border: 1px solid var(--exp-fg);
  color: var(--exp-fg);
  padding: 6px 10px;
  cursor: pointer;
  font-family: inherit;
  font-size: 0.85rem;
  border-radius: 4px;
}
button:hover:not(:disabled) {
  background: var(--exp-hover);
}
button:disabled {
  opacity: 0.45;
  cursor: default;
}
button.secondary,
button.sm {
  font-size: 0.78rem;
}

/* ── Inputs, selects, textareas ─────────────────────────────────────────────── */
.derived-input,
.derived-expr,
.derived-var-name,
.derived-sig-search {
  background: var(--exp-bg2);
  color: var(--exp-fg);
  border: 1px solid var(--exp-border);
  border-radius: 4px;
  padding: 7px 10px;
  font-family: inherit;
  font-size: 0.88rem;
  outline: none;
  width: 100%;
}
.derived-input:focus,
.derived-expr:focus,
.derived-var-name:focus,
.derived-sig-search:focus {
  border-color: var(--exp-fg-bright);
}
.derived-expr {
  resize: vertical;
  min-height: 70px;
}
.derived-var-name {
  padding: 6px 8px;
}
.derived-var-name.inp-error {
  border-color: #ff4444;
}
.derived-sig-search.has-selection {
  border-color: var(--exp-accent, var(--exp-fg-bright));
  color: var(--exp-fg-bright);
}

/* ── Variable rows ──────────────────────────────────────────────────────────── */
.derived-empty {
  color: var(--exp-fg-dim);
  font-size: 0.85rem;
  font-style: italic;
  padding: 6px 0;
}

.derived-var-section {
  position: relative;
}

.derived-var-row {
  display: grid;
  grid-template-columns: 90px 20px 68px 1fr 30px;
  align-items: center;
  gap: 6px;
}

.derived-transform-select {
  background: var(--exp-bg2);
  color: var(--exp-fg-bright);
  border: 1px solid var(--exp-border);
  border-radius: 4px;
  padding: 5px 4px;
  font-family: monospace;
  font-size: 0.8rem;
  outline: none;
  cursor: pointer;
  text-align: center;
}
.derived-transform-select:focus {
  border-color: var(--exp-fg-bright);
}

.derived-sig-picker {
  position: relative;
  width: 100%;
}

.derived-sig-dropdown {
  position: absolute;
  top: calc(100% + 3px);
  left: 0;
  right: 0;
  z-index: 3100;
  background: var(--exp-bg2);
  border: 1px solid var(--exp-border);
  border-radius: 4px;
  max-height: 220px;
  overflow-y: auto;
  box-shadow: 0 6px 18px rgba(0,0,0,0.45);
}

.derived-sig-item {
  display: flex;
  flex-direction: column;
  padding: 6px 10px;
  cursor: pointer;
  border-bottom: 1px solid var(--exp-border);
  gap: 1px;
}
.derived-sig-item:last-child { border-bottom: none; }
.derived-sig-item:hover,
.derived-sig-item.selected {
  background: var(--exp-highlight, rgba(255,255,255,0.08));
}

.derived-sig-channel {
  font-size: 0.87rem;
  color: var(--exp-fg);
  font-family: monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.derived-sig-file {
  font-size: 0.73rem;
  color: var(--exp-fg-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.derived-sig-no-results {
  padding: 10px;
  color: var(--exp-fg-dim);
  font-style: italic;
  font-size: 0.85rem;
}

.derived-eq {
  text-align: center;
  color: var(--exp-fg-dim);
  font-weight: bold;
}

.derived-icon-btn {
  background: transparent;
  border: 1px solid var(--exp-border);
  color: var(--exp-fg-dim);
  border-radius: 3px;
  padding: 4px 7px;
  cursor: pointer;
  font-size: 0.72rem;
  font-family: inherit;
}
.derived-icon-btn:hover {
  color: #ff6666;
  border-color: #ff4444;
  background: rgba(255, 80, 80, 0.1);
}

/* ── Reference panel ────────────────────────────────────────────────────────── */
.derived-ref {
  margin-top: 4px;
}

.derived-ref-toggle {
  font-size: 0.8rem;
  color: var(--exp-fg-dim2);
  cursor: pointer;
  user-select: none;
  list-style: none;
}
.derived-ref-toggle::marker,
.derived-ref-toggle::-webkit-details-marker { display: none; }
.derived-ref-toggle::before { content: '▶ '; font-size: 0.7rem; }
details[open] .derived-ref-toggle::before { content: '▼ '; }
.derived-ref-toggle:hover { color: var(--exp-fg); }

.derived-ref-body {
  margin-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 10px 12px;
  background: var(--exp-bg2);
  border: 1px solid var(--exp-border2);
  border-radius: 5px;
}

.derived-ref-row {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px;
  font-size: 0.8rem;
}

.derived-ref-row strong {
  min-width: 88px;
  color: var(--exp-fg-mid);
  flex-shrink: 0;
}

.derived-ref-row code {
  color: var(--exp-fg-bright);
  font-family: inherit;
  white-space: pre-wrap;
}

.derived-ref-note {
  color: var(--exp-fg-dim);
  font-size: 0.75rem;
}

/* ── Status messages ────────────────────────────────────────────────────────── */
.derived-status {
  padding: 9px 12px;
  border-radius: 5px;
  font-size: 0.86rem;
  font-weight: 600;
}
.derived-err {
  background: var(--exp-err-bg);
  border: 1px solid #660022;
  color: #ff6688;
}
.derived-ok {
  background: var(--exp-ok-bg);
  border: 1px solid #006644;
  color: var(--exp-fg-bright);
}

/* ── Actions row ────────────────────────────────────────────────────────────── */
.derived-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding-top: 4px;
  border-top: 1px solid var(--exp-border);
}
</style>
