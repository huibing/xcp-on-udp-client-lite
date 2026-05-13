<template>
  <div id="xcp-app" :class="`theme-${theme}`" :style="customThemeStyle">
    <nav class="sidebar" :class="{ collapsed: sidebarCollapsed }">
      <button class="sidebar-toggle" @click="toggleSidebar" :title="sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'">
        {{ sidebarCollapsed ? '»' : '«' }}
      </button>
      <div v-if="!sidebarCollapsed" class="brand">XCP Client</div>

      <router-link to="/"         :title="sidebarCollapsed ? 'Dashboard'  : ''">
        <span class="nav-icon">⊞</span><span class="nav-label">Dashboard</span>
      </router-link>
      <router-link to="/devices"  :title="sidebarCollapsed ? 'Devices'    : ''">
        <span class="nav-icon">⊙</span><span class="nav-label">Devices</span>
      </router-link>
      <router-link to="/experiment" :title="sidebarCollapsed ? 'Experiment' : ''">
        <span class="nav-icon">⚗</span><span class="nav-label">Experiment</span>
      </router-link>
      <router-link to="/compare"  :title="sidebarCollapsed ? 'Compare'    : ''">
        <span class="nav-icon">⇄</span><span class="nav-label">Compare</span>
      </router-link>
      <router-link to="/mf4"      :title="sidebarCollapsed ? 'MF4 Viewer' : ''">
        <span class="nav-icon">▶</span><span class="nav-label">MF4 Viewer</span>
      </router-link>
      <router-link to="/settings" class="settings-link" :title="sidebarCollapsed ? 'Settings' : ''">
        <span class="nav-icon">⚙</span><span class="nav-label">Settings</span>
      </router-link>
    </nav>
    <main class="content" :class="{ 'content-flush': $route.path === '/experiment' }">
      <router-view />
    </main>
  </div>
</template>

<script setup>
import { ref } from 'vue'
import { useTheme } from './composables/useTheme.js'
import { useFont } from './composables/useFont.js'

const { theme, customThemeStyle } = useTheme()
useFont() // initializes watchEffect that sets --exp-font / --exp-font-size on :root

const STORAGE_KEY = 'xcp-sidebar-collapsed'
const sidebarCollapsed = ref(localStorage.getItem(STORAGE_KEY) === 'true')

function toggleSidebar() {
  sidebarCollapsed.value = !sidebarCollapsed.value
  localStorage.setItem(STORAGE_KEY, sidebarCollapsed.value)
}
</script>

<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: var(--exp-font, 'Consolas', 'Courier New', monospace); font-size: var(--exp-font-size, 13px); }
#xcp-app { display: flex; height: 100vh; background: var(--exp-bg, #0a0a0a); color: var(--exp-fg, #00cc44); }

/* ── Global theme CSS variables (consumed by all pages) ──────────────────── */
#xcp-app {
  /* Green (default) */
  --exp-bg: #090909; --exp-bg2: #060606; --exp-bg3: #0d0d0d; --exp-bg4: #0a0a0a; --exp-bg5: #111111;
  --exp-fg: #00cc44; --exp-fg-bright: #00ff66; --exp-fg-mid: #00aa33;
  --exp-fg-dim: #556644; --exp-fg-dim2: #558844; --exp-fg-dim3: #446633; --exp-fg-dim4: #335522;
  --exp-border: #003311; --exp-border2: #002211; --exp-border3: #001a09;
  --exp-hover: #003311; --exp-ok-bg: #003322; --exp-err-bg: #330011;
  --exp-working-bg: #005522; --exp-primary-bg: #004433; --exp-cal-hover: #2a2a3f;
}
#xcp-app.theme-blue {
  --exp-bg: #1a2332; --exp-bg2: #152030; --exp-bg3: #1e293b; --exp-bg4: #141e2d; --exp-bg5: #1e2d3e;
  --exp-fg: #adb5bd; --exp-fg-bright: #e9ecef; --exp-fg-mid: #8fadc8;
  --exp-fg-dim: #6c7a8a; --exp-fg-dim2: #6c8090; --exp-fg-dim3: #5d7090; --exp-fg-dim4: #4d6080;
  --exp-border: #2d3f5a; --exp-border2: #253550; --exp-border3: #1e2d45;
  --exp-hover: #253550; --exp-ok-bg: #1a3a2a; --exp-err-bg: #3a1a22;
  --exp-working-bg: #1a3a5c; --exp-primary-bg: #1a3a5c; --exp-cal-hover: #253050;
}
#xcp-app.theme-light {
  --exp-bg: #ffffff; --exp-bg2: #f3f3f3; --exp-bg3: #ececec; --exp-bg4: #f8f8f8; --exp-bg5: #e8e8e8;
  --exp-fg: #333333; --exp-fg-bright: #000000; --exp-fg-mid: #0066b3;
  --exp-fg-dim: #666666; --exp-fg-dim2: #777777; --exp-fg-dim3: #888888; --exp-fg-dim4: #999999;
  --exp-border: #dddddd; --exp-border2: #e0e0e0; --exp-border3: #eeeeee;
  --exp-hover: #e8f0ff; --exp-ok-bg: #d4f5e2; --exp-err-bg: #fde8e8;
  --exp-working-bg: #c8e6fa; --exp-primary-bg: #d9eaf7; --exp-cal-hover: #e8eef8;
}

/* ── Sidebar ─────────────────────────────────────────────────────────────── */
.sidebar {
  width: 160px;
  min-width: 160px;
  background: var(--exp-bg2, #0d0d0d);
  border-right: 1px solid var(--exp-border, #00cc44);
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  transition: width 0.2s ease, min-width 0.2s ease;
  overflow: hidden;
}
.sidebar.collapsed {
  width: 44px;
  min-width: 44px;
}
.sidebar-toggle {
  background: none;
  border: none;
  border-bottom: 1px solid var(--exp-border, #003311);
  color: var(--exp-fg-mid, #00aa33);
  cursor: pointer;
  font-size: 0.9rem;
  padding: 10px 0;
  text-align: center;
  width: 100%;
  font-family: Consolas, monospace;
  flex-shrink: 0;
}
.sidebar-toggle:hover {
  background: var(--exp-border3, #001a09);
  color: var(--exp-fg-bright, #00ff66);
}
.brand {
  color: var(--exp-fg-bright, #00ff66);
  font-weight: bold;
  padding: 8px 16px 12px;
  border-bottom: 1px solid var(--exp-border, #003311);
  margin-bottom: 4px;
  white-space: nowrap;
  overflow: hidden;
}
.sidebar a {
  color: var(--exp-fg-mid, #00aa33);
  text-decoration: none;
  padding: 8px 0 8px 8px;
  display: flex;
  align-items: center;
  gap: 8px;
  white-space: nowrap;
  overflow: hidden;
}
.sidebar a:hover,
.sidebar a.router-link-active {
  background: var(--exp-border3, #001a09);
  color: var(--exp-fg-bright, #00ff66);
}
.nav-icon {
  flex-shrink: 0;
  width: 24px;
  text-align: center;
  font-size: 1rem;
}
.nav-label {
  overflow: hidden;
  text-overflow: ellipsis;
  transition: opacity 0.15s ease;
}
.sidebar.collapsed .nav-label {
  opacity: 0;
  pointer-events: none;
}
.settings-link {
  margin-top: auto;
  border-top: 1px solid var(--exp-border, #003311);
  padding-top: 8px !important;
}

/* ── Content area ────────────────────────────────────────────────────────── */
.content { flex: 1; overflow: auto; padding: 24px; }
.content.content-flush { padding: 0; overflow: hidden; display: flex; flex-direction: column; }
</style>
