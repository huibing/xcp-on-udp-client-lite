<template>
  <button
    type="button"
    class="bd-toggle"
    :class="{ on: state === true, editable, busy }"
    :disabled="!editable || busy"
    :title="editable ? 'Click to toggle' : ''"
    @click.stop="onClick"
  >
    <span class="track"><span class="knob" /></span>
    <span class="lbl">{{ label }}</span>
  </button>
</template>

<script setup>
import { computed } from 'vue'
import { booleanLabelFor } from '../display/boolean'

const props = defineProps({
  state: { default: null },
  // Declared for the uniform CardDisplay contract; this renderer uses `labels`.
  text: { default: '' },
  labels: { type: Object, default: () => ({ on: '1', off: '0' }) },
  editable: { type: Boolean, default: false },
  busy: { type: Boolean, default: false },
})
const emit = defineEmits(['select'])

const label = computed(() => booleanLabelFor(props.state, props.labels))

function onClick() {
  if (!props.editable || props.busy) return
  emit('select', props.state !== true)
}
</script>

<style scoped>
.bd-toggle {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  background: none;
  border: none;
  padding: 0;
  font: inherit;
  color: var(--exp-fg-dim);
  cursor: default;
}
.bd-toggle.editable { cursor: pointer; }
.bd-toggle.editable:hover .track { border-color: var(--exp-fg-mid); }
.bd-toggle:disabled { cursor: default; }

.track {
  position: relative;
  width: 38px;
  height: 18px;
  border-radius: 9px;
  background: var(--exp-bg3);
  border: 1px solid var(--exp-border2);
  transition: background 0.15s, border-color 0.15s;
  flex-shrink: 0;
}
.knob {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: var(--exp-fg-dim3);
  transition: transform 0.15s, background 0.15s;
}
.bd-toggle.on .track {
  background: var(--exp-ok-bg);
  border-color: var(--exp-fg-mid);
}
.bd-toggle.on .knob {
  transform: translateX(20px);
  background: var(--exp-fg-bright);
}
.lbl {
  font-size: 11px;
  letter-spacing: 0.3px;
  color: var(--exp-fg-dim2);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 84px;
}
.bd-toggle.on .lbl { color: var(--exp-fg-bright); }
.bd-toggle.busy { opacity: 0.5; }
</style>