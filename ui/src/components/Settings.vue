<template>
  <div class="settings-page">
    <div class="settings-header">
      <h2>⚙ Settings</h2>
      <p class="settings-sub">Global application settings. Changes take effect immediately.</p>
    </div>

    <div class="settings-card">
      <div class="settings-section-title">🎨 UI Theme</div>
      <p class="settings-hint">Choose a color theme for the entire application.</p>

      <div class="theme-picker">
        <button
          v-for="t in themeOptions" :key="t.id"
          class="theme-btn" :class="{ active: theme === t.id }"
          :title="t.label" @click="theme = t.id">
          <span class="theme-swatch" :style="{ background: t.bg, color: t.fg }">Aa</span>
          <span class="theme-name">{{ t.label }}</span>
        </button>
      </div>

      <div v-if="theme === 'custom'" class="theme-custom-row">
        <label class="settings-hint">Background</label>
        <input type="color" v-model="customBg" class="color-pick" />
        <label class="settings-hint" style="margin-left:12px">Foreground / Accent</label>
        <input type="color" v-model="customFg" class="color-pick" />
      </div>

      <div class="theme-preview">
        <span class="preview-label">Preview:</span>
        <span class="preview-chip preview-chip--fg">Foreground text</span>
        <span class="preview-chip preview-chip--bright">Accent / Bright</span>
        <span class="preview-chip preview-chip--dim">Dim text</span>
        <span class="preview-chip preview-chip--ok">✔ OK state</span>
        <span class="preview-chip preview-chip--err">✖ Error state</span>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useTheme } from '../composables/useTheme.js'

const { theme, customFg, customBg, themeOptions } = useTheme()
</script>

<style scoped>
.settings-page {
  max-width: 680px;
  margin: 0 auto;
  padding: 32px 24px;
  font-family: 'Consolas', 'Courier New', monospace;
  color: var(--exp-fg);
}

.settings-header { margin-bottom: 28px; }
.settings-header h2 { font-size: 20px; color: var(--exp-fg-bright, #00ff66); margin-bottom: 6px; }
.settings-sub { font-size: 13px; color: var(--exp-fg-dim, #556644); }

.settings-card {
  background: var(--exp-bg2, #060606);
  border: 1px solid var(--exp-border, #003311);
  border-radius: 8px;
  padding: 20px 24px;
  margin-bottom: 20px;
}

.settings-section-title {
  font-size: 14px;
  font-weight: 700;
  color: var(--exp-fg-bright, #00ff66);
  margin-bottom: 8px;
  letter-spacing: 0.04em;
}

.settings-hint {
  font-size: 12px;
  color: var(--exp-fg-dim, #556644);
  margin-bottom: 14px;
  display: block;
}

/* ── Theme Picker ────────────────────────────────────────────────────────── */
.theme-picker { display: flex; gap: 10px; flex-wrap: wrap; margin-bottom: 14px; }
.theme-btn {
  display: flex; flex-direction: column; align-items: center; gap: 5px;
  padding: 8px 12px; border-radius: 6px; cursor: pointer;
  border: 2px solid var(--exp-border2, #002211);
  background: var(--exp-bg3, #0d0d0d);
  transition: border-color 0.15s, background 0.15s;
}
.theme-btn:hover { border-color: var(--exp-border, #003311); }
.theme-btn.active { border-color: var(--exp-fg-bright, #00ff66); }
.theme-swatch {
  width: 56px; height: 32px; border-radius: 5px;
  display: flex; align-items: center; justify-content: center;
  font-size: 13px; font-weight: 700; font-family: monospace;
  border: 1px solid rgba(128,128,128,0.25);
}
.theme-name { font-size: 11px; color: var(--exp-fg-dim, #556644); white-space: nowrap; }
.theme-btn.active .theme-name { color: var(--exp-fg, #00cc44); }

.theme-custom-row {
  display: flex; align-items: center; gap: 8px;
  margin-bottom: 14px; flex-wrap: wrap;
}
.color-pick {
  width: 40px; height: 30px;
  border: 1px solid var(--exp-border, #003311);
  border-radius: 4px; padding: 2px;
  background: var(--exp-bg2, #060606);
  cursor: pointer;
}

/* ── Preview chips ───────────────────────────────────────────────────────── */
.theme-preview {
  display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
  margin-top: 6px;
}
.preview-label { font-size: 11px; color: var(--exp-fg-dim4, #335522); }
.preview-chip {
  font-size: 12px; padding: 3px 10px; border-radius: 4px;
  border: 1px solid var(--exp-border2, #002211);
}
.preview-chip--fg     { background: var(--exp-bg3); color: var(--exp-fg); }
.preview-chip--bright { background: var(--exp-bg3); color: var(--exp-fg-bright); }
.preview-chip--dim    { background: var(--exp-bg3); color: var(--exp-fg-dim); }
.preview-chip--ok     { background: var(--exp-ok-bg); color: var(--exp-fg); }
.preview-chip--err    { background: var(--exp-err-bg); color: #ff4466; }
</style>
