/** Tauri invoke() stand-in for the browser harness. Records calls + scripted replies. */
const state = {
  calls: [],
  written: null,      // content passed to write_text_file
  fileContent: null,  // content returned by read_text_file
  calValue: '1',      // last value passed to cal_write (kept for quick inspection)
  // Scripted calibration read-back, keyed "deviceId:idx". cal_write updates it,
  // so a write round-trip shows up on the card exactly like the real backend.
  calValues: { 'dev1:0': '1', 'dev1:1': '42.5', 'dev1:2': 'Enabled' },
}

export async function invoke(cmd, args) {
  state.calls.push({ cmd, args })

  switch (cmd) {
    case 'experiment_variables':
      return { success: true, measurements: MEASUREMENTS, calibrations: CALIBRATIONS }
    case 'experiment_events':
      return { success: true, devices: [{ config_device_id: 'dev1', events: [{ channel: 0, name: 'Event10ms', cycle_time_ms: 10 }] }] }
    case 'experiment_status':
      return { devices: [{ device_id: 'dev1', config_device_id: 'dev1', device_name: 'ECU1', connected: true, a2l_path: 'mock.a2l' }], daq_running_count: 0, recording_count: 0 }
    case 'list_devices':
      return []
    case 'experiment_cal_original':
      return { success: true, value: '0' }
    case 'cal_read': {
      const value = state.calValues[`${args?.deviceId}:${args?.idx}`] ?? '0'
      return { success: true, display_value: value, value: Number(value) }
    }
    case 'cal_write': {
      const value = String(args?.value ?? '0')
      state.calValue = value
      state.calValues[`${args?.deviceId}:${args?.idx}`] = value
      return { success: true }
    }
    case 'read_text_file':
      return { success: true, content: state.fileContent }
    case 'write_text_file':
      state.written = args?.content ?? null
      return { success: true }
    case 'experiment_daq_meta':
      return { success: false }
    default:
      return { success: true }
  }
}

const CALIBRATIONS = [
  { device_id: 'dev1', config_device_id: 'dev1', device_name: 'ECU1', idx: 0, name: 'Cal_EnableFan',
    type: 'U8', min: 0, max: 1, connected: true, characteristic_type: 'VALUE', unit: '', is_enum: false, enum_options: [] },
  { device_id: 'dev1', config_device_id: 'dev1', device_name: 'ECU1', idx: 1, name: 'Cal_PidKp',
    type: 'F32', min: 0, max: 100, connected: true, characteristic_type: 'VALUE', unit: '-', is_enum: false, enum_options: [] },
  { device_id: 'dev1', config_device_id: 'dev1', device_name: 'ECU1', idx: 2, name: 'Cal_EnumBool',
    type: 'U8', min: 0, max: 1, connected: true, characteristic_type: 'VALUE', unit: '', is_enum: true,
    enum_options: [{ raw: 0, text: 'Disabled' }, { raw: 1, text: 'Enabled' }] },
]

const MEASUREMENTS = [
  { device_id: 'dev1', config_device_id: 'dev1', device_name: 'ECU1', idx: 0, name: 'Flg_Boost',
    type: 'U8', min: 0, max: 1, connected: true, unit: '', is_enum: false, enum_options: [], address_hex: '0x20000000' },
  // Numeric measurement with A2L limits → gauge / level bar default to 0..8000 rpm
  { device_id: 'dev1', config_device_id: 'dev1', device_name: 'ECU1', idx: 1, name: 'Sig_Rpm',
    type: 'U16', min: 0, max: 8000, connected: true, unit: 'rpm', is_enum: false, enum_options: [], address_hex: '0x20000002' },
]

window.__MOCK__ = state
