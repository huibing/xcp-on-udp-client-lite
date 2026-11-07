<template>
  <span class="bd-text" :class="stateClass">{{ display }}</span>
</template>

<script setup>
import { computed } from 'vue'
import { booleanLabelFor } from '../display/boolean'

const props = defineProps({
  state: { default: null },
  text: { default: '' },
  labels: { type: Object, default: () => ({ on: '1', off: '0' }) },
})

const display = computed(() => {
  if (props.text !== undefined && props.text !== null && props.text !== '') return String(props.text)
  return booleanLabelFor(props.state, props.labels)
})

const stateClass = computed(() => ({
  on: props.state === true,
  off: props.state === false,
  unknown: props.state === null,
}))
</script>

<style scoped>
.bd-text {
  font-size: 18px;
  font-weight: 700;
  font-family: 'Consolas', monospace;
  color: var(--exp-fg-bright);
  line-height: 1.1;
}
.bd-text.off { color: var(--exp-fg-dim); }
.bd-text.unknown { color: var(--exp-fg-dim4); }
</style>