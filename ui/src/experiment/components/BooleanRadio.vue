<template>
  <div class="bd-radio" :class="{ editable, busy }" role="radiogroup">
    <button
      v-for="opt in options"
      :key="String(opt.value)"
      type="button"
      class="opt"
      :class="{ sel: state === opt.value }"
      role="radio"
      :aria-checked="state === opt.value"
      :disabled="!editable || busy"
      @click.stop="onPick(opt.value)"
    >
      <span class="dot" />
      <span class="txt">{{ opt.label }}</span>
    </button>
  </div>
</template>

<script setup>
import { computed } from 'vue'

const props = defineProps({
  state: { default: null },
  // Declared for the uniform CardDisplay contract; this renderer uses `labels`.
  text: { default: '' },
  labels: { type: Object, default: () => ({ on: '1', off: '0' }) },
  editable: { type: Boolean, default: false },
  busy: { type: Boolean, default: false },
})
const emit = defineEmits(['select'])

const options = computed(() => [
  { value: false, label: props.labels.off },
  { value: true, label: props.labels.on },
])

function onPick(value) {
  if (!props.editable || props.busy) return
  if (value === props.state) return
  emit('select', value)
}
</script>

<style scoped>
.bd-radio {
  display: inline-flex;
  gap: 4px;
  align-items: center;
}
.opt {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: transparent;
  border: 1px solid var(--exp-border2);
  border-radius: 10px;
  padding: 1px 7px 1px 5px;
  font: inherit;
  font-size: 10.5px;
  color: var(--exp-fg-dim3);
  cursor: default;
  transition: border-color 0.15s, color 0.15s, background 0.15s;
}
.bd-radio.editable .opt { cursor: pointer; }
.bd-radio.editable .opt:hover { border-color: var(--exp-fg-mid); color: var(--exp-fg-mid); }
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  border: 1px solid var(--exp-fg-dim4);
  background: transparent;
  flex-shrink: 0;
}
.opt.sel {
  color: var(--exp-fg-bright);
  border-color: var(--exp-fg-mid);
  background: var(--exp-ok-bg);
}
.opt.sel .dot {
  border-color: var(--exp-fg-bright);
  background: var(--exp-fg-bright);
}
.txt {
  max-width: 56px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bd-radio.busy { opacity: 0.5; }
</style>