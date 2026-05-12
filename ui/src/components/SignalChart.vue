<template>
  <div class="schart-wrap">
    <!-- Header: title + legend + live badge -->
    <div class="schart-header">
      <span class="schart-title">📈 Graph</span>
      <div class="schart-legend">
        <span v-for="s in signals" :key="s.key" class="schart-leg-item">
          <label class="schart-leg-color-lbl" :title="`Click to change color of ${s.name}`">
            <span class="schart-leg-line" :style="{ background: s.color }"></span>
            <input
              type="color"
              class="schart-leg-color-input"
              :value="s.color"
              @input="emit('color-change', { key: s.key, color: $event.target.value })"
            />
          </label>
          <span class="schart-leg-name">{{ s.name }}</span>
          <span v-if="showDevice && s.device_name" class="schart-leg-dev">[{{ s.device_name }}]</span>
        </span>
      </div>
      <span v-if="running" class="schart-live-badge">● LIVE</span>
      <button v-if="yOverrideMin !== null" class="schart-reset-y-btn" @click="resetYZoom" title="Reset Y zoom">↕ Reset Y</button>
      <button class="schart-close-btn" @click="$emit('close')" title="Close graph panel">✕</button>
    </div>

    <!-- Canvas area — mousedown/dblclick always active regardless of cursor mode -->
    <div class="schart-canvas-wrap"
         ref="canvasWrapEl"
         :style="{ height: chartHeight + 'px', cursor: canvasWrapCursor }"
         @mousemove.passive="onWrapMouseMove"
         @mouseleave="nearCursor = false"
         @mousedown.prevent="onCanvasDown"
         @dblclick.prevent="resetYZoom">
      <canvas ref="canvasEl"></canvas>
      <!-- Zoom selection rectangle — always visible during drag regardless of cursor mode -->
      <div v-if="zoomRect" class="schart-zoom-rect" :style="zoomRectStyle">
        <span v-if="zoomAxis" class="schart-zoom-axis-label">{{ zoomAxis === 'x' ? '↔ X zoom' : '↕ Y zoom' }}</span>
      </div>
      <!-- Draggable time cursor (MF4 viewer mode) -->
      <div v-if="showCursor"
           class="schart-cursor-overlay"
           ref="cursorOverlayEl">
        <div class="schart-cursor-line" :style="cursorLineStyle"></div>
        <div v-if="cursorValues.length > 0" class="schart-cursor-tooltip" :style="cursorTooltipStyle">
          <div class="schart-cursor-time">t = {{ localCursorTime !== null ? localCursorTime.toFixed(4) : '—' }} s</div>
          <div v-for="v in cursorValues" :key="v.key" class="schart-cursor-val">
            <span class="schart-cursor-swatch" :style="{ background: v.color }"></span>
            <span class="schart-cursor-name">{{ v.name }}</span>
            <span class="schart-cursor-num">{{ v.display }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- Height resize handle -->
    <div class="schart-resize-handle" @mousedown.prevent="onResizeDown" title="Drag to resize chart height"></div>

    <!-- Slider controls (live mode only) -->
    <div v-if="showWindowSlider" class="schart-controls">
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
  /** Override x-axis min (seconds). When both xMin and xMax are non-null, used directly in getXBounds(). */
  xMin: { type: Number, default: null },
  /** Override x-axis max (seconds). When both xMin and xMax are non-null, used directly in getXBounds(). */
  xMax: { type: Number, default: null },
  /** When false, hide the time-window slider (set false for static MF4 viewer mode). */
  showWindowSlider: { type: Boolean, default: true },
  /** Show a draggable time cursor with signal value tooltip. Disables Chart.js hover tooltip. */
  showCursor: { type: Boolean, default: false },
  /** External cursor time (seconds) for synchronized multi-chart mode. When provided, cursor syncs from parent. */
  cursorTime: { type: Number, default: null },
})

const emit = defineEmits(['close', 'cursor-move', 'color-change', 'zoom-x'])
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

// ── Cursor state ──────────────────────────────────────────────────────────────
const cursorOverlayEl = ref(null)
const canvasWrapEl = ref(null)     // always available; used for bounds in zoom + cursor
const cursorPx = ref(null)         // pixel x within canvas, null = not placed yet
const localCursorTime = ref(null)  // internal cursor time (seconds), source of truth
// Reactive snapshots of chart internals — updated after every doUpdate() call
const chartXRange = ref({ min: 0, max: 1 })
const chartPlotArea = ref({ left: 0, right: 1, top: 0, bottom: 1 })
const nearCursor = ref(false)      // mouse is within grab distance of the time cursor
const cursorDragging = ref(false)  // time cursor drag in progress

const canvasWrapCursor = computed(() => {
  if (props.showCursor && nearCursor.value) return cursorDragging.value ? 'grabbing' : 'grab'
  return 'crosshair'
})

function onWrapMouseMove(e) {
  if (!props.showCursor || cursorPx.value === null || !canvasWrapEl.value) {
    nearCursor.value = false
    return
  }
  const rect = canvasWrapEl.value.getBoundingClientRect()
  nearCursor.value = Math.abs(e.clientX - rect.left - cursorPx.value) <= 8
}

// ── Zoom state ────────────────────────────────────────────────────────────────
const zoomRect = ref(null)       // { x1, y1, x2, y2 } px in overlay
const zoomAxis = ref(null)       // 'x' | 'y' | null (determined by drag direction)
const yOverrideMin = ref(null)   // manual Y-axis override; null = auto
const yOverrideMax = ref(null)

// ── Chart height (user-resizable) ─────────────────────────────────────────────
const chartHeight = ref(240)
const MIN_CHART_H = 80
const MAX_CHART_H = 1200

function onResizeDown(e) {
  const startY = e.clientY
  const startH = chartHeight.value
  document.documentElement.style.cursor = 'ns-resize'
  function onMove(e) {
    chartHeight.value = Math.max(MIN_CHART_H, Math.min(MAX_CHART_H, startH + e.clientY - startY))
  }
  function onUp() {
    document.documentElement.style.cursor = ''
    document.removeEventListener('mousemove', onMove)
    document.removeEventListener('mouseup', onUp)
  }
  document.addEventListener('mousemove', onMove)
  document.addEventListener('mouseup', onUp)
}

function getElapsed() {
  if (props.startEpoch == null) return 0
  return (Date.now() - props.startEpoch) / 1000
}

/** Compute x-axis bounds in seconds */
function getXBounds() {
  // Explicit range override (MF4 viewer two-cursor mode)
  if (props.xMin !== null && props.xMax !== null) {
    return { xMin: props.xMin, xMax: props.xMax }
  }
  const ws = windowSeconds.value
  if (props.running && props.startEpoch != null) {
    const now = getElapsed()
    const xMax = Math.max(ws, now)
    return { xMin: xMax - ws, xMax }
  }
  // Static fallback: show last windowSeconds of data
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

  // Apply y-axis bounds with margin (or manual override)
  if (yOverrideMin.value !== null && yOverrideMax.value !== null) {
    chart.options.scales.y.min = yOverrideMin.value
    chart.options.scales.y.max = yOverrideMax.value
  } else if (yMin === Infinity) {
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
  // Sync reactive chart area refs so cursor stays accurate after range changes
  chartXRange.value = { min: chart.scales.x.min, max: chart.scales.x.max }
  chartPlotArea.value = { ...chart.chartArea }
  _refreshCursorPx()
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

// ── Cursor helpers ────────────────────────────────────────────────────────────
/**
 * Return fresh chart bounds directly from the Chart.js instance.
 * Falls back to cached refs if chart is not ready.
 * Calling this at drag-start prevents stale-cache issues after container resize.
 */
function _liveChartArea() {
  if (chart?.chartArea && chart.scales?.x) {
    const ca = chart.chartArea
    const x = chart.scales.x
    return { ca: { ...ca }, xMin: x.min, xMax: x.max }
  }
  const ca = chartPlotArea.value
  const { min: xMin, max: xMax } = chartXRange.value
  return { ca, xMin, xMax }
}

/** Convert a canvas pixel x to time using current chart x range */
function _pixelToTime(px, ca, xMin, xMax) {
  // Accept live bounds as arguments so callers can pass fresh values
  ca = ca ?? chartPlotArea.value
  xMin = xMin ?? chartXRange.value.min
  xMax = xMax ?? chartXRange.value.max
  const pct = Math.max(0, Math.min(1, (px - ca.left) / (ca.right - ca.left)))
  return xMin + pct * (xMax - xMin)
}

/** Re-derive cursor pixel position from stored localCursorTime after a range change */
function _refreshCursorPx() {
  if (localCursorTime.value === null) { cursorPx.value = null; return }
  const ca = chartPlotArea.value
  const { min, max } = chartXRange.value
  const span = max - min
  if (span <= 0) { cursorPx.value = null; return }
  const pct = (localCursorTime.value - min) / span
  cursorPx.value = (pct >= 0 && pct <= 1) ? ca.left + pct * (ca.right - ca.left) : null
}

/** Place cursor at a client-space X coordinate */
function _placeCursorAt(clientX) {
  const rect = canvasWrapEl.value?.getBoundingClientRect()
  if (!rect || !chart) return
  const px = clientX - rect.left
  // Read live bounds to guard against stale cache after container resize
  const { ca, xMin, xMax } = _liveChartArea()
  const clamped = Math.max(ca.left, Math.min(ca.right, px))
  cursorPx.value = clamped
  localCursorTime.value = _pixelToTime(clamped, ca, xMin, xMax)
  emit('cursor-move', localCursorTime.value)
}

function onCanvasDown(e) {
  e.preventDefault()
  if (!canvasWrapEl.value) return

  // Refresh cache from live chart to fix stale-area bug after container resize
  const { ca: liveCa, xMin: liveXMin, xMax: liveXMax } = _liveChartArea()
  chartPlotArea.value = liveCa
  chartXRange.value = { min: liveXMin, max: liveXMax }

  const wrapRect = canvasWrapEl.value.getBoundingClientRect()
  const clickX = e.clientX - wrapRect.left
  const isNearCursor = props.showCursor && cursorPx.value !== null && Math.abs(clickX - cursorPx.value) <= 8

  if (isNearCursor) {
    // ── Cursor-drag mode (only when cursor visible) ─────────────────────────
    cursorDragging.value = true
    document.documentElement.style.cursor = 'grabbing'
    _placeCursorAt(e.clientX)
    function onMoveCursor(e) { if (cursorDragging.value) _placeCursorAt(e.clientX) }
    function onUpCursor() {
      cursorDragging.value = false
      document.documentElement.style.cursor = ''
      document.removeEventListener('mousemove', onMoveCursor)
      document.removeEventListener('mouseup', onUpCursor)
    }
    document.addEventListener('mousemove', onMoveCursor)
    document.addEventListener('mouseup', onUpCursor)
    return
  }

  // ── Zoom-select / click-to-place-cursor mode ───────────────────────────────
  // Capture bounds at drag-start so all zoom calculations are consistent
  const { ca: snapCa, xMin: snapXMin, xMax: snapXMax } = _liveChartArea()
  const startX = clickX
  const startY = e.clientY - wrapRect.top
  let zooming = false

  function onMoveZoom(e) {
    const x = e.clientX - wrapRect.left
    const y = e.clientY - wrapRect.top
    const dx = Math.abs(x - startX)
    const dy = Math.abs(y - startY)
    if (!zooming && dx + dy > 5) zooming = true
    if (zooming) {
      zoomRect.value = { x1: startX, y1: startY, x2: x, y2: y }
      zoomAxis.value = dx >= dy ? 'x' : 'y'
    }
  }

  function onUpZoom(e) {
    document.removeEventListener('mousemove', onMoveZoom)
    document.removeEventListener('mouseup', onUpZoom)

    if (!zooming) {
      // Plain click: place cursor (only if cursor is visible)
      if (props.showCursor) _placeCursorAt(e.clientX)
      return
    }

    const r = zoomRect.value
    const axis = zoomAxis.value
    zoomRect.value = null
    zoomAxis.value = null
    if (!r || !axis) return

    if (axis === 'x') {
      const t1 = _pixelToTime(Math.min(r.x1, r.x2), snapCa, snapXMin, snapXMax)
      const t2 = _pixelToTime(Math.max(r.x1, r.x2), snapCa, snapXMin, snapXMax)
      if (t2 - t1 > 1e-6) emit('zoom-x', { xMin: t1, xMax: t2 })
    } else {
      // Y zoom: resolve pixels → data values via chart scale
      const y1px = Math.min(r.y1, r.y2)
      const y2px = Math.max(r.y1, r.y2)
      if (chart && y2px - y1px > 5) {
        const vBottom = chart.scales.y.getValueForPixel(y2px)
        const vTop = chart.scales.y.getValueForPixel(y1px)
        if (vBottom !== undefined && vTop !== undefined && Math.abs(vTop - vBottom) > 1e-10) {
          yOverrideMin.value = Math.min(vBottom, vTop)
          yOverrideMax.value = Math.max(vBottom, vTop)
          doUpdate()
        }
      }
    }
  }

  document.addEventListener('mousemove', onMoveZoom)
  document.addEventListener('mouseup', onUpZoom)
}

function resetYZoom() {
  if (yOverrideMin.value !== null) {
    yOverrideMin.value = null
    yOverrideMax.value = null
    doUpdate()
  }
}

/** Style for the zoom-selection rubber-band rectangle */
const zoomRectStyle = computed(() => {
  if (!zoomRect.value || !zoomAxis.value) return { display: 'none' }
  const { x1, y1, x2, y2 } = zoomRect.value
  const ca = chartPlotArea.value
  if (zoomAxis.value === 'x') {
    const left = Math.max(ca.left, Math.min(x1, x2))
    const right = Math.min(ca.right, Math.max(x1, x2))
    return { left: `${left}px`, top: `${ca.top}px`, width: `${Math.max(0, right - left)}px`, height: `${ca.bottom - ca.top}px` }
  } else {
    const top = Math.max(ca.top, Math.min(y1, y2))
    const bottom = Math.min(ca.bottom, Math.max(y1, y2))
    return { left: `${ca.left}px`, top: `${top}px`, width: `${ca.right - ca.left}px`, height: `${Math.max(0, bottom - top)}px` }
  }
})

/** Binary search: value of signal `key` at the given time (nearest sample) */
function getValueAtTime(key, time) {
  const h = props.historyStore.get(key)
  if (!h || h.times.length === 0) return null
  if (time < h.times[0] || time > h.times[h.times.length - 1]) return null
  let lo = 0, hi = h.times.length - 1
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (h.times[mid] < time) lo = mid + 1
    else hi = mid
  }
  if (lo > 0 && Math.abs(h.times[lo - 1] - time) < Math.abs(h.times[lo] - time)) lo--
  return h.values[lo]
}

const cursorLineStyle = computed(() => {
  if (cursorPx.value === null) return { display: 'none' }
  const ca = chartPlotArea.value
  return { left: `${cursorPx.value}px`, top: `${ca.top}px`, height: `${ca.bottom - ca.top}px` }
})

const cursorTooltipStyle = computed(() => {
  if (cursorPx.value === null) return { display: 'none' }
  const w = canvasEl.value?.offsetWidth ?? 400
  const showLeft = cursorPx.value > w * 0.62
  return {
    top: `${chartPlotArea.value.top + 6}px`,
    left: showLeft ? 'auto' : `${cursorPx.value + 14}px`,
    right: showLeft ? `${w - cursorPx.value + 14}px` : 'auto',
  }
})

const cursorValues = computed(() => {
  if (localCursorTime.value === null) return []
  return props.signals.map(sig => {
    const v = getValueAtTime(sig.key, localCursorTime.value)
    if (v === null) return null
    const text = vtabLookup(sig.key, v)
    const display = text != null
      ? `${text} (${v.toFixed(0)})`
      : Number.isFinite(v) ? v.toFixed(4) : '—'
    return { key: sig.key, name: sig.name, color: sig.color, display }
  }).filter(Boolean)
})

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
          enabled: !props.showCursor,
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
          ticks: {
            color: props.tickColor,
            maxTicksLimit: 6,
            callback: v => {
              const n = Number(v)
              if (Math.abs(n) >= 1e5 || (Math.abs(n) < 1e-3 && n !== 0)) return n.toExponential(1)
              return parseFloat(n.toPrecision(4)).toString()
            },
          },
          border: { color: props.gridColor },
          afterFit(scale) { scale.width = 58 },  // fixed Y-axis width keeps plot area stable
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

watch([() => props.xMin, () => props.xMax], () => {
  doUpdate()
})

watch(() => props.showCursor, v => {
  if (chart) {
    chart.options.plugins.tooltip.enabled = !v
    chart.update('none')
  }
  if (!v) {
    cursorPx.value = null
    localCursorTime.value = null
  } else if (props.cursorTime !== null && props.cursorTime !== undefined) {
    // Restore cursor to last known position when re-enabling
    localCursorTime.value = props.cursorTime
    _refreshCursorPx()
  }
})

// Sync cursor from parent when in multi-chart synchronized mode
watch(() => props.cursorTime, v => {
  if (v !== null && v !== undefined) {
    localCursorTime.value = v
    _refreshCursorPx()
  } else {
    localCursorTime.value = null
    cursorPx.value = null
  }
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

.schart-leg-color-lbl {
  position: relative;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  border-radius: 2px;
  padding: 4px 2px;
}

.schart-leg-color-lbl:hover .schart-leg-line {
  outline: 1px solid rgba(255, 255, 255, 0.45);
  outline-offset: 2px;
}

.schart-leg-color-input {
  position: absolute;
  opacity: 0;
  width: 100%;
  height: 100%;
  top: 0;
  left: 0;
  cursor: pointer;
  padding: 0;
  border: none;
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
  padding: 8px 8px 4px;
  background: var(--exp-bg);
  position: relative;
  min-height: 80px;
}

.schart-canvas-wrap canvas {
  width: 100% !important;
  height: 100% !important;
}

/* ── Resize handle ───────────────────────────────────────────────────────────── */
.schart-resize-handle {
  height: 5px;
  cursor: ns-resize;
  background: var(--exp-border2, #001a08);
  transition: background 0.15s;
  user-select: none;
}

.schart-resize-handle:hover {
  background: var(--exp-fg-dim3, #335522);
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

/* ── Cursor overlay ──────────────────────────────────────────────────────────── */
.schart-cursor-overlay {
  position: absolute;
  inset: 0;
  z-index: 10;
  pointer-events: none;  /* canvas-wrap handles all pointer events */
}

.schart-cursor-line {
  position: absolute;
  width: 1px;
  background: rgba(255, 204, 0, 0.85);
  box-shadow: 0 0 5px rgba(255, 204, 0, 0.35);
  pointer-events: none;
  z-index: 11;
}

.schart-cursor-tooltip {
  position: absolute;
  z-index: 12;
  background: rgba(0, 0, 0, 0.9);
  border: 1px solid rgba(255, 204, 0, 0.4);
  border-radius: 5px;
  padding: 6px 10px;
  pointer-events: none;
  min-width: 130px;
  max-width: 280px;
}

.schart-cursor-time {
  font-size: 11px;
  color: #ffcc00;
  font-weight: 700;
  margin-bottom: 5px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.schart-cursor-val {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  color: #cccccc;
  margin-top: 2px;
}

.schart-cursor-swatch {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.schart-cursor-name {
  color: var(--exp-fg-dim3);
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 120px;
}

.schart-cursor-num {
  font-variant-numeric: tabular-nums;
  color: #ffffff;
  white-space: nowrap;
}

/* ── Zoom selection rectangle ─────────────────────────────────────────────── */
.schart-zoom-rect {
  position: absolute;
  pointer-events: none;
  background: rgba(0, 220, 80, 0.07);
  border: 1px solid rgba(0, 220, 80, 0.45);
  box-sizing: border-box;
  z-index: 9;
  display: flex;
  align-items: flex-start;
  justify-content: center;
}

.schart-zoom-axis-label {
  font-size: 10px;
  font-weight: 700;
  color: rgba(0, 220, 80, 0.9);
  background: rgba(0,0,0,0.6);
  padding: 1px 4px;
  border-radius: 2px;
  letter-spacing: 0.4px;
  margin-top: 3px;
  white-space: nowrap;
  pointer-events: none;
  user-select: none;
}

/* Y-zoom reset button in chart header */
.schart-reset-y-btn {
  background: transparent;
  border: 1px solid rgba(0, 220, 80, 0.35);
  color: rgba(0, 220, 80, 0.7);
  border-radius: 3px;
  padding: 2px 7px;
  font-size: 10px;
  cursor: pointer;
  white-space: nowrap;
}

.schart-reset-y-btn:hover {
  color: var(--exp-fg-bright, #00ff66);
  border-color: var(--exp-fg-bright, #00ff66);
}
</style>
