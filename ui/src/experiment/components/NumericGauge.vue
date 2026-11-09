<template>
  <div
    class="num-gauge"
    :class="{ unknown: fraction === null }"
    :title="`${formatBound(cfg.min)} … ${formatBound(cfg.max)}${rangeNote}`"
  >
    <svg class="gauge" viewBox="0 0 100 76" role="img" :aria-label="ariaLabel">
      <!-- Track: the full 240° sweep -->
      <path class="track" :d="arcPath(0, 1)" />
      <!-- Value: the covered part of the sweep -->
      <path v-if="valuePath" class="fill" :d="valuePath" :stroke="cfg.color" />
      <!-- Ticks at 0/25/50/75/100 % -->
      <line
        v-for="t in TICKS"
        :key="t"
        class="tick"
        :x1="tickPoint(t, R - 8).x"
        :y1="tickPoint(t, R - 8).y"
        :x2="tickPoint(t, R - 4.5).x"
        :y2="tickPoint(t, R - 4.5).y"
      />
      <text class="bound" x="17" y="74" text-anchor="middle">{{ formatBound(cfg.min) }}</text>
      <text class="bound" x="83" y="74" text-anchor="middle">{{ formatBound(cfg.max) }}</text>
    </svg>
    <div class="readout">
      <span class="value">{{ text }}</span>
      <span v-if="unit" class="unit">{{ unit }}</span>
    </div>
  </div>
</template>

<script setup>
import { computed } from 'vue'
import { normalizeConfig, numericFraction } from '../display/numeric'

const props = defineProps({
  // Declared for the uniform CardDisplay contract; the gauge is read-only.
  state: { default: null },
  labels: { type: Object, default: () => ({ on: '1', off: '0' }) },
  editable: { type: Boolean, default: false },
  busy: { type: Boolean, default: false },
  // Formatted readout (already rounded by Experiment.vue) and its unit.
  text: { default: '' },
  unit: { type: String, default: '' },
  // Raw number + resolved config ({ min, max, color, … }) from numeric.js.
  value: { default: null },
  config: { type: Object, default: () => ({}) },
})

const CX = 50
const CY = 50
const R = 32
/** The sweep starts bottom-left and runs clockwise 240° to bottom-right. */
const START_DEG = 150
const SWEEP_DEG = 240
const TICKS = [0, 0.25, 0.5, 0.75, 1]

const cfg = computed(() => normalizeConfig(props.config))
const fraction = computed(() => numericFraction(props.value, cfg.value))
const valuePath = computed(() => (fraction.value ? arcPath(0, fraction.value) : ''))
const rangeNote = computed(() => (cfg.value.rangeSource === 'user' ? '' : ' (auto range)'))
const ariaLabel = computed(
  () => `${props.text}${props.unit ? ' ' + props.unit : ''} of ${formatBound(cfg.value.min)}..${formatBound(cfg.value.max)}`
)

function pointAt(fractionOfSweep, radius) {
  const deg = START_DEG + SWEEP_DEG * fractionOfSweep
  const rad = (deg * Math.PI) / 180
  return {
    x: (CX + radius * Math.cos(rad)).toFixed(2),
    y: (CY + radius * Math.sin(rad)).toFixed(2),
  }
}

function tickPoint(fractionOfSweep, radius) {
  return pointAt(fractionOfSweep, radius)
}

/** SVG arc from one fraction of the sweep to another (empty when degenerate). */
function arcPath(from, to) {
  if (!(to > from)) return ''
  const start = pointAt(from, R)
  const end = pointAt(to, R)
  const largeArc = SWEEP_DEG * (to - from) > 180 ? 1 : 0
  return `M ${start.x} ${start.y} A ${R} ${R} 0 ${largeArc} 1 ${end.x} ${end.y}`
}

function formatBound(num) {
  if (!Number.isFinite(num)) return ''
  if (Number.isInteger(num)) return String(num)
  const abs = Math.abs(num)
  if (abs >= 100) return num.toFixed(0)
  if (abs >= 1) return num.toFixed(1)
  return num.toFixed(2)
}
</script>

<style scoped>
.num-gauge {
  position: relative;
  width: 104px;
  max-width: 100%;
  flex-shrink: 0;
}
.gauge {
  display: block;
  width: 100%;
  height: auto;
  overflow: visible;
}
.track {
  fill: none;
  stroke: var(--exp-border);
  stroke-width: 7;
  stroke-linecap: round;
}
.fill {
  fill: none;
  stroke-width: 7;
  stroke-linecap: round;
  transition: stroke 0.2s;
}
.tick {
  stroke: var(--exp-border2);
  stroke-width: 1;
}
.bound {
  font-size: 7px;
  font-family: 'Consolas', monospace;
  fill: var(--exp-fg-dim3);
}
.readout {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 12%;
  display: flex;
  align-items: baseline;
  justify-content: center;
  gap: 3px;
  pointer-events: none;
}
.value {
  font-size: 14px;
  font-weight: 700;
  font-family: 'Consolas', monospace;
  color: var(--exp-fg-bright);
  line-height: 1;
  max-width: 74px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.unit {
  font-size: 9px;
  color: var(--exp-fg-dim3);
}
.num-gauge.unknown .value { color: var(--exp-fg-dim4); }
</style>