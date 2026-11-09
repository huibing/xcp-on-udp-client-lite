/**
 * Tauri event stand-in for the browser harness.
 *
 * `listen()` behaves like the real API (returns an unlisten function) and the
 * harness can push synthetic events with `emit()` — that is how the DAQ stream
 * is faked so gauge / level-bar cards can be watched while they update.
 */
const listeners = new Map()

export async function listen(name, handler) {
  if (!listeners.has(name)) listeners.set(name, new Set())
  listeners.get(name).add(handler)
  return () => {
    listeners.get(name)?.delete(handler)
  }
}

export async function emit(name, payload) {
  for (const handler of listeners.get(name) ?? []) {
    try {
      handler({ event: name, payload, id: 0 })
    } catch (err) {
      console.error(`[harness] listener for ${name} failed`, err)
    }
  }
}
// Handy for poking the page from the devtools console:
//   __emit('daq-data', { device_id: 'dev1', values: { Sig_Rpm: 1234 } })
if (typeof window !== 'undefined') window.__emit = emit
