import { createApp } from 'vue'
import { createRouter, createWebHashHistory } from 'vue-router'
import App from './App.vue'

// Page components (stubs — Phase 7 will implement each fully)
import Dashboard from './components/Dashboard.vue'
import DeviceManager from './components/DeviceManager.vue'
import Calibration from './components/Calibration.vue'
import DAQ from './components/DAQ.vue'
import Experiment from './components/Experiment.vue'
import Checksum from './components/Checksum.vue'
import Compare from './components/Compare.vue'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: Dashboard },
    { path: '/devices', component: DeviceManager },
    { path: '/calibration', component: Calibration },
    { path: '/daq', component: DAQ },
    { path: '/experiment', component: Experiment },
    { path: '/checksum', component: Checksum },
    { path: '/compare', component: Compare },
  ],
})

const app = createApp(App)
app.use(router)
app.mount('#app')
