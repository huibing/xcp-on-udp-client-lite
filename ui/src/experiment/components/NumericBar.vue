<template>
  <div
    class="num-bar"
    :class="[cfg.orientation, { unknown: fraction === null }]"
    :title="`${formatBound(cfg.min)} … ${formatBound(cfg.max)}${rangeNote}`"
  >
    <div class="battery">
      <div class="body">
        <div class="fill" :style="fillStyle" />
        <!-- Optional cell dividers, so the bar reads like a battery gauge -->
        <span v-for="i in cellLines" :key="i" class="cell" :style="cellStyle(i)" />
      </div>
      <div class="cap" />
    </div>
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
  // Declared for the uniform CardDisplay contract; the level bar is read-only.
  state: { default: null },
  labels: { type: Object, default: () => ({ on: '1', off: '0' }) },
  editable: { type: Boolean, default: false },
  busy: { type: Boolean, default: false },
  // Formatted readout (already rounded by Experiment.vue) and its unit.
  text: { default: '' },
  unit: { type: String, default: '' },
  // Raw number + resolved config ({ min, max, orientation, color, segments }).
  value: { default: null },
  config: { type: Object, default: () => ({}) },
})

const cfg = computed(() => normalizeConfig(props.config))
const fraction = computed(() => numericFraction(props.value, cfg.value))
const vertical = computed(() => cfg.value.orientation === 'vertical')
const rangeNote = computed(() => (cfg.value.rangeSource === 'user' ? '' : ' (auto range)'))

/** Number of interior divider lines: `segments` cells need `segments - 1`. */
const cellLines = computed(() => Math.max(0, cfg.value.segments - 1))

const fillStyle = computed(() => {
  const size = `${((fraction.value ?? 0) * 100).toFixed(1)}%`
  return vertical.value
    ? { height: size, background: cfg.value.color }
    : { width: size, background: cfg.value.color }
})

/** Divider i sits at i/segments of the body, measured from the fill origin. */
function cellStyle(i) {
  const pos = `${((i / cfg.value.segments) * 100).toFixed(2)}%`
  return vertical.value ? { bottom: pos } : { left: pos }
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
.num-bar {
  display: flex;
  gap: 7px;
  min-width: 0;
  flex-shrink: 0;
}
.num-bar.horizontal {
  align-items: center;
}
.num-bar.vertical {
  flex-direction: column;
  align-items: center;
  gap: 4px;
}
.battery {
  display: flex;
  flex-shrink: 0;
}
.horizontal .battery {
  flex-direction: row;
  align-items: center;
}
.vertical .battery {
  flex-direction: column;
  align-items: center;
}
.body {
  position: relative;
  overflow: hidden;
  background: var(--exp-bg3);
  border: 1.5px solid var(--exp-border2);
  border-radius: 3px;
  box-sizing: border-box;
}
.horizontal .body { width: 62px; height: 19px; }
.vertical .body { width: 19px; height: 52px; }
.cap {
  background: var(--exp-border2);
  border-radius: 1px;
}
.horizontal .cap {
  width: 3px;
  height: 8px;
  margin-left: 2px;
  border-radius: 0 2px 2px 0;
}
.vertical .cap {
  order: -1;
  width: 8px;
  height: 3px;
  margin-bottom: 2px;
  border-radius: 2px 2px 0 0;
}
.fill {
  position: absolute;
  transition: width 0.15s linear, height 0.15s linear, background 0.2s;
}
.horizontal .fill {
  left: 0;
  top: 0;
  bottom: 0;
}
.vertical .fill {
  left: 0;
  right: 0;
  bottom: 0;
}
.cell {
  position: absolute;
  background: var(--exp-bg5);
  opacity: 0.85;
}
.horizontal .cell {
  top: 0;
  bottom: 0;
  width: 1px;
}
.vertical .cell {
  left: 0;
  right: 0;
  height: 1px;
}
.readout {
  display: flex;
  align-items: baseline;
  gap: 3px;
  min-width: 0;
}
.value {
  font-size: 13px;
  font-weight: 700;
  font-family: 'Consolas', monospace;
  color: var(--exp-fg-bright);
  line-height: 1.1;
  max-width: 78px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.unit {
  font-size: 9px;
  color: var(--exp-fg-dim3);
}
.num-bar.unknown .value { color: var(--exp-fg-dim4); }
</style>