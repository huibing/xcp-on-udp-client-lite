<template>
  <button
    type="button"
    class="bd-bulb"
    :class="{ on: state === true, off: state === false, editable, busy }"
    :disabled="!editable || busy"
    :title="editable ? 'Click to toggle' : ''"
    @click.stop="onClick"
  >
    <svg class="lamp" viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M12 2.6a6.4 6.4 0 0 0-3.7 11.6c.5.4.8 1 .9 1.6l.1.6h5.4l.1-.6c.1-.6.4-1.2.9-1.6A6.4 6.4 0 0 0 12 2.6Z"
        fill="currentColor"
      />
      <rect x="9.4" y="17.3" width="5.2" height="1.9" rx="0.9" fill="currentColor" opacity="0.85" />
      <rect x="10.2" y="20" width="3.6" height="1.5" rx="0.75" fill="currentColor" opacity="0.6" />
    </svg>
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
.bd-bulb {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  background: none;
  border: none;
  padding: 0;
  font: inherit;
  cursor: default;
  color: var(--exp-fg-dim4);
}
.bd-bulb.editable { cursor: pointer; }
.bd-bulb.editable:hover { color: var(--exp-fg-mid); }
.bd-bulb:disabled { cursor: default; }

.lamp {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
  transition: color 0.15s, filter 0.15s;
}
.bd-bulb.on { color: #f5b301; }
.bd-bulb.on .lamp {
  filter: drop-shadow(0 0 5px rgba(245, 179, 1, 0.75));
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
.bd-bulb.on .lbl { color: var(--exp-fg-bright); }
.bd-bulb.busy { opacity: 0.5; }
</style>