<template>
  <div class="card-display" :class="{ interactive }" @click="onWrapClick">
    <component
      :is="style.component"
      :state="state"
      :text="text"
      :labels="labels"
      :editable="interactive"
      :busy="busy"
      @select="$emit('select', $event)"
    />
    <span v-if="showUnit" class="unit">{{ unit }}</span>
  </div>
</template>

<script setup>
import { computed } from 'vue'
import { resolveStyle } from '../display/styles'

const props = defineProps({
  kind: { type: String, default: 'unknown' },
  styleId: { type: String, default: '' },
  state: { default: null },
  text: { default: '' },
  labels: { type: Object, default: () => ({ on: '1', off: '0' }) },
  unit: { type: String, default: '' },
  editable: { type: Boolean, default: false },
  busy: { type: Boolean, default: false },
})
defineEmits(['select'])

const style = computed(() => resolveStyle(props.kind, props.styleId))

/** A renderer may be a plain display (text) or an input (toggle/radio/bulb). */
const interactive = computed(() => props.editable && style.value.interactive)

/** Units belong to the numeric readout; a switch or lamp does not carry one. */
const showUnit = computed(() => !!props.unit && style.value.id === 'text')

/**
 * The calibration card opens its edit modal on click. An interactive renderer
 * owns the click instead, otherwise toggling would also pop the modal.
 */
function onWrapClick(event) {
  if (interactive.value) event.stopPropagation()
}
</script>

<style scoped>
.card-display {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  max-width: 100%;
}
.card-display.interactive { cursor: pointer; }
.unit {
  font-size: 11px;
  color: var(--exp-fg-dim3);
  align-self: baseline;
}
</style>