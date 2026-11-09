<template>
  <div v-if="fields.length > 0" class="display-options" @click.stop>
    <span
      class="opt-btn"
      :class="{ active: open, dirty }"
      :title="`${style.label} options`"
      @click.stop="open = !open"
      >⚙</span
    >
    <div v-if="open" class="opt-backdrop" @click="open = false" />
    <div v-if="open" class="opt-menu" @click.stop>
      <div class="opt-title">{{ style.label }} options</div>
      <label v-for="f in fields" :key="f.key" class="opt-row">
        <span class="opt-label">{{ f.label }}</span>
        <input
          v-if="f.type === 'number'"
          class="opt-input"
          type="number"
          :step="f.step ?? 1"
          :value="valueOf(f)"
          :placeholder="placeholderOf(f)"
          @change="onNumber(f, $event)"
        />
        <select
          v-else-if="f.type === 'select'"
          class="opt-input"
          :value="valueOf(f)"
          @change="set(f.key, $event.target.value)"
        >
          <option v-for="c in f.choices" :key="c.value" :value="c.value">{{ c.label }}</option>
        </select>
        <input
          v-else-if="f.type === 'color'"
          class="opt-color"
          type="color"
          :value="valueOf(f)"
          @input="set(f.key, $event.target.value)"
        />
      </label>
      <div v-if="hasAutoRange" class="opt-hint">
        Empty min/max = A2L limits ({{ formatBound(autoRange.min) }} … {{ formatBound(autoRange.max) }})
      </div>
      <button v-if="dirty" type="button" class="opt-reset" @click="reset">Reset to defaults</button>
    </div>
  </div>
</template>

<script setup>
import { computed, ref } from 'vue'
import { optionsFor, resolveStyle } from '../display/styles'
import { numericRange } from '../display/numeric'

const props = defineProps({
  kind: { type: String, default: 'unknown' },
  styleId: { type: String, default: '' },
  /** The variable, only read for its A2L limits (auto-range placeholders). */
  variable: { type: Object, default: null },
  /** Raw user config: `v.display_config` (every key optional). */
  modelValue: { type: Object, default: () => ({}) },
})
const emit = defineEmits(['update:modelValue'])

const open = ref(false)

const style = computed(() => resolveStyle(props.kind, props.styleId))
const fields = computed(() => optionsFor(props.kind, props.styleId))

/** What the bounds would be if the user leaves min/max empty. */
const autoRange = computed(() => numericRange(props.variable, {}))
const hasAutoRange = computed(() => fields.value.some(f => f.auto))

const dirty = computed(() => fields.value.some(f => {
  const raw = props.modelValue?.[f.key]
  return raw !== undefined && raw !== null && raw !== ''
}))

/** Effective value shown in a control (unset keys fall back to the default). */
function valueOf(f) {
  const raw = props.modelValue?.[f.key]
  if (raw === undefined || raw === null || raw === '') {
    return f.type === 'number' ? '' : (f.default ?? '')
  }
  return raw
}

/** Placeholder for an unset field: the range bound or default it would inherit. */
function placeholderOf(f) {
  if (f.auto === 'min') return formatBound(autoRange.value.min)
  if (f.auto === 'max') return formatBound(autoRange.value.max)
  return f.placeholder ?? ''
}

/** Write one key; an empty string deletes it so the default applies again. */
function set(key, value) {
  const next = { ...(props.modelValue || {}) }
  if (value === '' || value === null || value === undefined) delete next[key]
  else next[key] = value
  emit('update:modelValue', next)
}

function onNumber(f, event) {
  const raw = event.target.value
  if (String(raw).trim() === '') return set(f.key, '')
  const num = Number(raw)
  if (!Number.isFinite(num)) return
  set(f.key, num)
}

function reset() {
  emit('update:modelValue', {})
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
.display-options {
  position: relative;
  display: inline-flex;
}
.opt-btn {
  cursor: pointer;
  font-size: 11px;
  line-height: 1;
  padding: 2px 4px;
  border-radius: 3px;
  color: var(--exp-fg-dim4);
  transition: color 0.15s, background 0.15s;
}
.opt-btn:hover,
.opt-btn.active {
  color: var(--exp-fg);
  background: var(--exp-border3);
}
.opt-btn.dirty { color: var(--exp-fg-mid); }
.opt-backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;
}
.opt-menu {
  position: absolute;
  top: 20px;
  right: 0;
  z-index: 50;
  width: 214px;
  padding: 5px;
  background: var(--exp-bg3);
  border: 1px solid var(--exp-border);
  border-radius: 6px;
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.45);
}
.opt-title {
  font-size: 9px;
  letter-spacing: 0.6px;
  text-transform: uppercase;
  color: var(--exp-fg-dim4);
  padding: 3px 6px 5px;
}
.opt-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 3px 6px;
}
.opt-label {
  font-size: 11px;
  color: var(--exp-fg-dim2);
  white-space: nowrap;
}
.opt-input {
  width: 84px;
  padding: 3px 5px;
  font: inherit;
  font-size: 11px;
  font-family: 'Consolas', monospace;
  color: var(--exp-fg-bright);
  background: var(--exp-bg5);
  border: 1px solid var(--exp-border2);
  border-radius: 4px;
  outline: none;
}
.opt-input:focus { border-color: var(--exp-fg-mid); }
.opt-input::placeholder { color: var(--exp-fg-dim4); }
.opt-color {
  width: 34px;
  height: 20px;
  padding: 0;
  background: none;
  border: 1px solid var(--exp-border2);
  border-radius: 4px;
  cursor: pointer;
}
.opt-hint {
  padding: 4px 6px 2px;
  font-size: 9.5px;
  line-height: 1.4;
  color: var(--exp-fg-dim4);
}
.opt-reset {
  width: 100%;
  margin-top: 4px;
  padding: 4px 6px;
  font: inherit;
  font-size: 10.5px;
  color: var(--exp-fg-dim2);
  background: none;
  border: 1px solid var(--exp-border2);
  border-radius: 4px;
  cursor: pointer;
}
.opt-reset:hover {
  color: var(--exp-fg-bright);
  border-color: var(--exp-fg-mid);
}
</style>