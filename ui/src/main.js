import { createApp } from 'vue'
import { createRouter, createWebHashHistory } from 'vue-router'
import App from './App.vue'

import Dashboard from './components/Dashboard.vue'
import DeviceManager from './components/DeviceManager.vue'
import Experiment from './components/Experiment.vue'
import Compare from './components/Compare.vue'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: Dashboard },
    { path: '/devices', component: DeviceManager },
    { path: '/experiment', component: Experiment },
    { path: '/compare', component: Compare },
  ],
})

const app = createApp(App)
app.use(router)
app.mount('#app')
