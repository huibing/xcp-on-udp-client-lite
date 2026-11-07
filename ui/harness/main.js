import { createApp } from 'vue'
import App from './App.vue'

// Seed a v1-shaped saved state (no display_style anywhere) so the page boots
// straight into cards — this is what an existing user's localStorage looks like.
const seed = {
  expTitle: 'E2E',
  tabCounter: 2,
  activeTabId: 'tab1',
  tabs: [{
    id: 'tab1',
    title: 'T1',
    graphedKeys: [],
    variables: [
      { kind: 'calibration', name: 'Cal_EnableFan', device_id: 'dev1', config_device_id: 'dev1',
        device_name: 'ECU1', idx: 0, type: 'U8', min: 0, max: 1, connected: true, calType: 'VALUE' },
      { kind: 'calibration', name: 'Cal_PidKp', device_id: 'dev1', config_device_id: 'dev1',
        device_name: 'ECU1', idx: 1, type: 'F32', min: 0, max: 100, connected: true, calType: 'VALUE' },
      { kind: 'calibration', name: 'Cal_EnumBool', device_id: 'dev1', config_device_id: 'dev1',
        device_name: 'ECU1', idx: 2, type: 'U8', min: 0, max: 1, connected: true, calType: 'VALUE',
        is_enum: true, enum_options: [{ raw: 0, text: 'Disabled' }, { raw: 1, text: 'Enabled' }] },
      { kind: 'measurement', name: 'Flg_Boost', device_id: 'dev1', config_device_id: 'dev1',
        device_name: 'ECU1', idx: 0, type: 'U8', min: 0, max: 1, connected: true },
    ],
  }],
  signalColors: {},
}
localStorage.setItem('exp_tauri_state_v3', JSON.stringify(seed))

createApp(App).mount('#app')
