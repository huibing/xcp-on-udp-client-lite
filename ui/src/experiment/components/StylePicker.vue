<template>
  <div class="style-picker" @click.stop>
    <span
      class="picker-btn"
      :class="{ active: open }"
      :title="`Card display style (${styles.length} available)`"
      @click.stop="toggle"
      >◈</span
    >
    <div v-if="open" class="picker-backdrop" @click="open = false" />
    <div v-if="open" class="picker-menu" @click.stop>
      <div class="picker-title">Display style</div>
      <button
        v-for="s in styles"
        :key="s.id"
        type="button"
        class="picker-item"
        :class="{ sel: s.id === modelValue }"
        @click="pick(s.id)"
      >
        <span class="ico">{{ s.icon }}</span>
        <span class="txt">{{ s.label }}</span>
        <span v-if="s.id === modelValue" class="chk">✓</span>
      </button>
    </div>
  </div>
</template>

<script setup>
import { computed, ref } from 'vue'
import { stylesFor } from '../display/styles'

const props = defineProps({
  kind: { type: String, default: 'unknown' },
  modelValue: { type: String, default: '' },
})
const emit = defineEmits(['update:modelValue'])

const open = ref(false)
const styles = computed(() => stylesFor(props.kind))

function toggle() {
  open.value = !open.value
}

function pick(id) {
  open.value = false
  if (id !== props.modelValue) emit('update:modelValue', id)
}
</script>

<style scoped>
.style-picker {
  position: relative;
  display: inline-flex;
}
.picker-btn {
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
  padding: 2px 4px;
  border-radius: 3px;
  color: var(--exp-fg-dim4);
  transition: color 0.15s, background 0.15s;
}
.picker-btn:hover,
.picker-btn.active {
  color: var(--exp-fg);
  background: var(--exp-border3);
}
.picker-backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;
}
.picker-menu {
  position: absolute;
  top: 20px;
  right: 0;
  z-index: 50;
  min-width: 132px;
  padding: 4px;
  background: var(--exp-bg3);
  border: 1px solid var(--exp-border);
  border-radius: 6px;
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.45);
}
.picker-title {
  font-size: 9px;
  letter-spacing: 0.6px;
  text-transform: uppercase;
  color: var(--exp-fg-dim4);
  padding: 3px 6px 4px;
}
.picker-item {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  padding: 5px 6px;
  background: none;
  border: none;
  border-radius: 4px;
  font: inherit;
  font-size: 11.5px;
  color: var(--exp-fg-dim2);
  text-align: left;
  cursor: pointer;
}
.picker-item:hover {
  background: var(--exp-hover);
  color: var(--exp-fg-bright);
}
.picker-item.sel { color: var(--exp-fg-bright); }
.ico {
  width: 16px;
  text-align: center;
  font-size: 11px;
  opacity: 0.9;
}
.txt { flex: 1; }
.chk { color: var(--exp-fg-bright); }
</style>