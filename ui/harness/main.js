import { createApp } from 'vue'
import { emit } from '@tauri-apps/api/event'
import App from './App.vue'

// Seed a saved state so the page boots straight into cards. No display_style /
// display_config anywhere: this is what an existing user's localStorage (and a
// v1 experiment file) looks like, so the defaults must render fine.
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
      { kind: 'measurement', name: 'Sig_Rpm', device_id: 'dev1', config_device_id: 'dev1',
        device_name: 'ECU1', idx: 1, type: 'U16', min: 0, max: 8000, unit: 'rpm', connected: true },
    ],
  }],
  signalColors: {},
}
localStorage.setItem('exp_tauri_state_v3', JSON.stringify(seed))

createApp(App).mount('#app')

// Fake DAQ stream: a triangle wave on Sig_Rpm and a slow toggle on Flg_Boost, so
// gauge / level-bar cards visibly track a live value without hardware.
let tick = 0
setInterval(() => {
  tick = (tick + 1) % 40
  const rpm = Math.round((tick < 20 ? tick : 40 - tick) / 20 * 8000)
  emit('daq-data', {
    device_id: 'dev1',
    values: { Sig_Rpm: rpm, Flg_Boost: tick % 2 === 0 ? 1 : 0 },
  })
}, 250)