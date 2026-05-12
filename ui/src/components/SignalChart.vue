<template>
  <div class="schart-wrap">
    <!-- Header: title + legend + live badge -->
    <div class="schart-header">
      <span class="schart-title">📈 Graph</span>
      <div class="schart-legend">
        <span v-for="s in signals" :key="s.key" class="schart-leg-item">
          <span class="schart-leg-line" :style="{ background: s.color }"></span>
          <span class="schart-leg-name">{{ s.name }}</span>
          <span v-if="showDevice && s.device_name" class="schart-leg-dev">[{{ s.device_name }}]</span>
        </span>
      </div>
      <span v-if="running" class="schart-live-badge">● LIVE</span>
      <button class="schart-close-btn" @click="$emit('close')" title="Close graph panel">✕</button>
    </div>

    <!-- Canvas area -->
    <div class="schart-canvas-wrap">
      <canvas ref="canvasEl"></canvas>
    </div>

    <!-- Slider controls -->
    <div class="schart-controls">
      <span class="schart-win-label">{{ windowLabel }}</span>
      <input
        type="range"
        class="schart-slider"
        :min="0"
        :max="WINDOW_OPTIONS.length - 1"
        v-model.number="windowIdx"
      />
      <span class="schart-win-hint">time window</span>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import {
  Chart,
  LineController,
  LineElement,
  PointElement,
  LinearScale,
  Tooltip,
} from 'chart.js'

Chart.register(LineController, LineElement, PointElement, LinearScale, Tooltip)

// ── Constants ─────────────────────────────────────────────────────────────────
const WINDOW_OPTIONS = [5, 10, 20, 30, 60, 120, 300]
const UPDATE_MS = 100          // chart refresh interval while running
const MAX_RENDER_PTS = 2000    // max points per dataset to render (downsample if more)

// ── Props ─────────────────────────────────────────────────────────────────────
const props = defineProps({
  /** Array of { key: "device_id:name", name: string, device_name: string, color: string } */
  signals: { type: Array, default: () => [] },
  /** Plain Map<key, { times: number[], values: number[] }> — not reactive, polled by timer */
  historyStore: { type: Object, required: true },
  /** true when DAQ is active (auto-scroll x-axis) */
  running: { type: Boolean, default: false },
  /** epoch ms when current DAQ session started (null if never started) */
  startEpoch: { type: Number, default: null },
  /** Chart grid line color */
  gridColor: { type: String, default: 'rgba(0,200,100,0.1)' },
  /** Chart tick label color */
  tickColor: { type: String, default: '#00aa33' },
  /**
   * Optional Value-to-Text maps keyed by signal key.
   * Shape: { [signalKey]: Array<{ key: number, text: string }> }
   * When present, tooltip shows text label alongside raw value for enum signals.
   */
  vtabMaps: { type: Object, default: () => ({}) },
})

const emit = defineEmits(['close'])
const canvasEl = ref(null)

const windowIdx = ref((() => {
  const saved = parseInt(localStorage.getItem('schart_window_idx') ?? '3')
  return isNaN(saved) ? 3 : Math.max(0, Math.min(saved, WINDOW_OPTIONS.length - 1))
})())

const windowSeconds = computed(() => WINDOW_OPTIONS[windowIdx.value] ?? 30)

const windowLabel = computed(() => {
  const s = windowSeconds.value
  return s >= 60 ? `${s / 60} min` : `${s} s`
})

/** Show device name in legend only when there are signals from multiple devices */
const showDevice = computed(() => {
  const devs = new Set(props.signals.map(s => s.device_name))
  return devs.size > 1
})

// Persist window preference
watch(windowIdx, v => localStorage.setItem('schart_window_idx', String(v)))

// ── Chart instance ────────────────────────────────────────────────────────────
let chart = null
let updateTimer = null

function getElapsed() {
  if (props.startEpoch == null) return 0
  return (Date.now() - props.startEpoch) / 1000
}

/** Compute x-axis bounds in seconds */
function getXBounds() {
  const ws = windowSeconds.value
  if (props.running && props.startEpoch != null) {
    const now = getElapsed()
    const xMax = Math.max(ws, now)
    return { xMin: xMax - ws, xMax }
  }
  // Static: find latest time across all signals
  let maxT = ws
  for (const sig of props.signals) {
    const h = props.historyStore.get(sig.key)
    if (h && h.times.length > 0) {
      const last = h.times[h.times.length - 1]
      if (last > maxT) maxT = last
    }
  }
  return { xMin: Math.max(0, maxT - ws), xMax: maxT }
}

/** Binary search: largest index where arr[i] < target */
function bsearchLeft(arr, target) {
  let lo = 0, hi = arr.length
  while (lo < hi) {
    const mid = (lo + hi) >>> 1
    if (arr[mid] < target) lo = mid + 1
    else hi = mid
  }
  return Math.max(0, lo - 1)
}

/** Downsample pts array to at most maxPts using Nth-point algorithm */
function downsample(times, values, startI, endI, maxPts) {
  const count = endI - startI
  if (count <= maxPts) {
    const pts = new Array(count)
    for (let i = 0; i < count; i++) pts[i] = { x: times[startI + i], y: values[startI + i] }
    return pts
  }
  const step = count / maxPts
  const pts = []
  for (let i = 0; i < maxPts; i++) {
    const idx = startI + Math.round(i * step)
    pts.push({ x: times[idx], y: values[idx] })
  }
  return pts
}

/** Main chart update — called by timer and on-demand */
function doUpdate() {
  if (!chart) return
  const { xMin, xMax } = getXBounds()
  let yMin = Infinity, yMax = -Infinity

  chart.data.datasets.forEach((ds, i) => {
    const sig = props.signals[i]
    if (!sig) { ds.data = []; return }
    const h = props.historyStore.get(sig.key)
    if (!h || h.times.length === 0) { ds.data = []; return }

    // Find slice of data in [xMin-1 … xMax+1] (1s buffer for smooth edges)
    const startI = bsearchLeft(h.times, xMin - 1)
    let endI = h.times.length
    for (let j = startI; j < h.times.length; j++) {
      if (h.times[j] > xMax + 1) { endI = j + 1; break }
    }

    ds.data = downsample(h.times, h.values, startI, endI, MAX_RENDER_PTS)

    // Compute y min/max from points strictly within the visible window
    for (let j = startI; j < endI; j++) {
      const t = h.times[j]
      if (t < xMin || t > xMax) continue
      const v = h.values[j]
      if (v < yMin) yMin = v
      if (v > yMax) yMax = v
    }
  })

  // Apply x-axis bounds
  chart.options.scales.x.min = xMin
  chart.options.scales.x.max = xMax

  // Apply y-axis bounds with margin
  if (yMin === Infinity) {
    chart.options.scales.y.min = -1
    chart.options.scales.y.max = 1
  } else if (Math.abs(yMax - yMin) < 1e-9) {
    chart.options.scales.y.min = yMin - 1
    chart.options.scales.y.max = yMax + 1
  } else {
    const margin = (yMax - yMin) * 0.12
    chart.options.scales.y.min = yMin - margin
    chart.options.scales.y.max = yMax + margin
  }

  chart.update('none')
}

/** Resolve a raw numeric value to vtab text for a given signal key */
function vtabLookup(signalKey, rawValue) {
  const entries = props.vtabMaps[signalKey]
  if (!entries || entries.length === 0) return null
  const entry = entries.find(e => Math.abs(e.key - rawValue) < 0.5)
  return entry ? entry.text : null
}

/** Rebuild Chart.js datasets to match current signals list */
function buildDatasets() {
  if (!chart) return
  chart.data.datasets = props.signals.map(sig => ({
    label: sig.name,
    data: [],
    borderColor: sig.color,
    backgroundColor: 'transparent',
    borderWidth: 1.5,
    pointRadius: 0,
    tension: 0.1,
    parsing: false,
  }))
  chart.update('none')
}

/** Apply theme colors to existing chart */
function applyColors() {
  if (!chart) return
  chart.options.scales.x.grid.color = props.gridColor
  chart.options.scales.x.ticks.color = props.tickColor
  chart.options.scales.x.border.color = props.gridColor
  chart.options.scales.y.grid.color = props.gridColor
  chart.options.scales.y.ticks.color = props.tickColor
  chart.options.scales.y.border.color = props.gridColor
  chart.update('none')
}

function startLoop() {
  stopLoop()
  updateTimer = setInterval(doUpdate, UPDATE_MS)
}

function stopLoop() {
  if (updateTimer) { clearInterval(updateTimer); updateTimer = null }
}

function createChart() {
  if (!canvasEl.value || chart) return
  chart = new Chart(canvasEl.value, {
    type: 'line',
    data: { datasets: [] },
    options: {
      animation: false,
      responsive: true,
      maintainAspectRatio: false,
      parsing: false,
      plugins: {
        legend: { display: false },
        tooltip: {
          mode: 'index',
          intersect: false,
          backgroundColor: 'rgba(0,0,0,0.85)',
          titleColor: props.tickColor,
          bodyColor: '#cccccc',
          borderColor: props.gridColor,
          borderWidth: 1,
          callbacks: {
            title: items => items.length ? `t = ${Number(items[0].parsed.x).toFixed(2)} s` : '',
            label: item => {
              const raw = Number(item.parsed.y)
              const sig = props.signals[item.datasetIndex]
              const text = sig ? vtabLookup(sig.key, raw) : null
              if (text != null) return ` ${item.dataset.label}: ${text} (${raw.toFixed(0)})`
              return ` ${item.dataset.label}: ${raw.toFixed(4)}`
            },
          },
        },
      },
      scales: {
        x: {
          type: 'linear',
          min: 0,
          max: windowSeconds.value,
          grid: { color: props.gridColor },
          ticks: {
            color: props.tickColor,
            maxTicksLimit: 8,
            callback: v => `${Number(v).toFixed(1)}s`,
          },
          border: { color: props.gridColor },
        },
        y: {
          type: 'linear',
          min: -1,
          max: 1,
          grid: { color: props.gridColor },
          ticks: { color: props.tickColor, maxTicksLimit: 6 },
          border: { color: props.gridColor },
        },
      },
    },
  })
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────
onMounted(() => {
  nextTick(() => {
    createChart()
    buildDatasets()
    doUpdate()
    if (props.running) startLoop()
  })
})

onUnmounted(() => {
  stopLoop()
  if (chart) { chart.destroy(); chart = null }
})

// ── Watchers ──────────────────────────────────────────────────────────────────
watch(() => props.running, r => {
  if (r) startLoop()
  else { stopLoop(); doUpdate() }
})

watch(() => props.signals, () => {
  buildDatasets()
  doUpdate()
}, { deep: true })

watch(windowSeconds, () => {
  doUpdate()
})

watch([() => props.gridColor, () => props.tickColor], () => {
  applyColors()
})
</script>

<style scoped>
.schart-wrap {
  background: var(--exp-bg2);
  border: 1px solid var(--exp-border);
  border-radius: 6px;
  margin-top: 14px;
  overflow: hidden;
  flex-shrink: 0;
}

.schart-close-btn {
  margin-left: auto;
  background: none;
  border: none;
  color: var(--exp-fg-dim3);
  font-size: 14px;
  cursor: pointer;
  line-height: 1;
  padding: 2px 5px;
  border-radius: 3px;
  transition: color 0.15s, background 0.15s;
}
.schart-close-btn:hover {
  color: #ff5555;
  background: rgba(255, 85, 85, 0.12);
}

/* ── Header ────────────────────────────────────────────────────────────────── */
.schart-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 12px;
  border-bottom: 1px solid var(--exp-border);
  background: var(--exp-bg3);
  min-height: 34px;
}

.schart-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--exp-fg-bright);
  white-space: nowrap;
  letter-spacing: 0.5px;
}

.schart-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  flex: 1;
  overflow: hidden;
}

.schart-leg-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
}

.schart-leg-line {
  width: 20px;
  height: 2px;
  border-radius: 1px;
  flex-shrink: 0;
}

.schart-leg-name {
  color: var(--exp-fg);
  white-space: nowrap;
}

.schart-leg-dev {
  color: var(--exp-fg-dim3);
  font-size: 10px;
}

.schart-live-badge {
  font-size: 11px;
  font-weight: 700;
  color: #00ff66;
  letter-spacing: 0.5px;
  white-space: nowrap;
  animation: schart-blink 1.2s ease-in-out infinite;
}

@keyframes schart-blink {
  0%, 100% { opacity: 1; }
  50%       { opacity: 0.35; }
}

/* ── Canvas area ───────────────────────────────────────────────────────────── */
.schart-canvas-wrap {
  height: 240px;
  padding: 8px 8px 4px;
  background: var(--exp-bg);
  position: relative;
}

.schart-canvas-wrap canvas {
  width: 100% !important;
  height: 100% !important;
}

/* ── Controls ──────────────────────────────────────────────────────────────── */
.schart-controls {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 5px 12px;
  border-top: 1px solid var(--exp-border);
  background: var(--exp-bg3);
}

.schart-win-label {
  font-size: 12px;
  color: var(--exp-fg-bright);
  min-width: 48px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.schart-slider {
  flex: 1;
  accent-color: var(--exp-fg);
  cursor: pointer;
  height: 4px;
}

.schart-win-hint {
  font-size: 11px;
  color: var(--exp-fg-dim3);
  white-space: nowrap;
}
</style>
