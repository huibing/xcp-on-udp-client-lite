<template>
  <div id="xcp-app">
    <nav class="sidebar">
      <div class="brand">XCP Client</div>
      <router-link to="/">Dashboard</router-link>
      <router-link to="/devices">Devices</router-link>
      <router-link to="/experiment">Experiment</router-link>
      <router-link to="/compare">Compare</router-link>
    </nav>
    <main class="content" :class="{ 'content-flush': $route.path === '/experiment' }">
      <router-view />
    </main>
  </div>
</template>

<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { background: #0a0a0a; color: #00cc44; font-family: 'Consolas', 'Courier New', monospace; }
#xcp-app { display: flex; height: 100vh; }
.sidebar { width: 160px; background: #0d0d0d; border-right: 1px solid #00cc44; padding: 16px 0; display: flex; flex-direction: column; gap: 4px; }
.brand { color: #00ff66; font-weight: bold; padding: 8px 16px 16px; border-bottom: 1px solid #003311; margin-bottom: 8px; }
.sidebar a { color: #00aa33; text-decoration: none; padding: 8px 16px; display: block; }
.sidebar a:hover, .sidebar a.router-link-active { background: #001a09; color: #00ff66; }
.content { flex: 1; overflow: auto; padding: 24px; }
.content.content-flush { padding: 0; overflow: hidden; display: flex; flex-direction: column; }
</style>
