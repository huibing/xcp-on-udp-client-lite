/**
 * Boolean-specific value helpers shared by every boolean renderer.
 *
 * Values reach the UI in three different shapes and all three must work:
 *  - measurements: a raw number from the DAQ stream (`liveValues`)
 *  - calibrations: a display string from `cal_read` (`display_value`)
 *  - enum booleans: the option text instead of a number
 */

function enumOptions(v) {
  return Array.isArray(v?.enum_options) ? v.enum_options : []
}

const TRUE_WORDS = new Set(['true', 'on', 'yes', 'high', 'active', 'enabled', '1'])
const FALSE_WORDS = new Set(['false', 'off', 'no', 'low', 'inactive', 'disabled', '0'])

/**
 * Labels used by the switch / radio / bulb renderers.
 * Enum booleans carry their own text (e.g. "OFF"/"ON", "Disabled"/"Enabled");
 * plain 0..1 parameters fall back to "0"/"1".
 */
export function booleanLabels(v) {
  const opts = enumOptions(v)
  if (opts.length === 2) {
    const off = opts.find(o => Number(o.raw) === 0)
    const on = opts.find(o => Number(o.raw) !== 0)
    if (off && on) return { on: String(on.text), off: String(off.text) }
  }
  return { on: '1', off: '0' }
}

/**
 * Normalise a raw or display value to `true` / `false` / `null` (unknown).
 * @param {*} raw
 * @param {object} [v] variable, used to map enum text back to its raw value
 */
export function booleanState(raw, v) {
  if (raw === undefined || raw === null) return null

  if (typeof raw === 'boolean') return raw

  if (typeof raw === 'number') {
    return Number.isFinite(raw) ? raw !== 0 : null
  }

  const text = String(raw).trim()
  if (text === '' || text === '—' || text === '-' || text === 'N/A') return null

  const opts = enumOptions(v)
  if (opts.length) {
    const hit = opts.find(o => String(o.text).trim().toLowerCase() === text.toLowerCase())
    if (hit) return Number(hit.raw) !== 0
  }

  const num = Number(text)
  if (Number.isFinite(num)) return num !== 0

  const word = text.toLowerCase()
  if (TRUE_WORDS.has(word)) return true
  if (FALSE_WORDS.has(word)) return false
  return null
}

/**
 * The value to hand to `cal_write` for a requested boolean state.
 *
 * Enum parameters must be written with their *display text* (`cal_write`
 * resolves it through the COMPU_METHOD); plain scalars are written numerically,
 * which also keeps the working-page HEX patch byte-exact.
 */
export function booleanWriteValue(v, next) {
  const opts = enumOptions(v)
  if (v?.is_enum && opts.length === 2) {
    const target = opts.find(o => (Number(o.raw) !== 0) === !!next)
    if (target) return String(target.text)
  }
  return next ? 1 : 0
}

/** Short label for a state, used by the switch / bulb renderers. */
export function booleanLabelFor(state, labels) {
  if (state === true) return labels.on
  if (state === false) return labels.off
  return '—'
}