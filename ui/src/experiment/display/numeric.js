
/**
 * Numeric-specific value helpers shared by every numeric renderer
 * (gauge, level bar, and anything added later).
 *
 * Three things every numeric renderer needs:
 *   - a number parsed out of whatever the backend handed over,
 *   - the *display range* (min/max) that number is drawn against,
 *   - the fraction of the range it currently covers.
 *
 * The range is the only part that carries user configuration. It is stored per
 * variable as `v.display_config = { min, max, orientation, color, segments }`
 * (see styles.js for the field descriptors). Every key is optional: a missing
 * `min`/`max` means "use the A2L LOWER_LIMIT/UPPER_LIMIT", and only when the
 * A2L has no usable limits either does the generic 0..100 fallback kick in.
 *
 * Nothing here imports a .vue file, so it can be unit-tested with plain node.
 */

/** Used when neither the user nor the A2L gives us a range. */
export const DEFAULT_NUMERIC_RANGE = Object.freeze({ min: 0, max: 100 })

/** Style option defaults, also what "Reset" in the options popup restores. */
export const NUMERIC_DEFAULTS = Object.freeze({
  orientation: 'horizontal',
  color: '#00ff66',
  segments: 0,
})

/** A value coming from the DAQ stream / cal_read, normalised to a number. */
export function numericValue(raw) {
  if (raw === undefined || raw === null) return null
  if (typeof raw === 'number') return Number.isFinite(raw) ? raw : null
  const text = String(raw).trim()
  if (text === '' || text === '—' || text === '-' || text === 'N/A') return null
  const num = Number(text)
  return Number.isFinite(num) ? num : null
}

function finiteOrNull(value) {
  if (value === undefined || value === null || value === '') return null
  const num = Number(value)
  return Number.isFinite(num) ? num : null
}

/**
 * Limits declared by the A2L (`LOWER_LIMIT` / `UPPER_LIMIT`, physical values).
 * Returns null when they are missing or degenerate (e.g. 0..0 on hand-written
 * A2L files), so callers can fall back.
 */
export function a2lRange(v) {
  const min = finiteOrNull(v?.min)
  const max = finiteOrNull(v?.max)
  if (min === null || max === null || max <= min) return null
  return { min, max }
}

/**
 * Resolve the range the renderer draws against.
 * @param {object} v variable (for the A2L limits)
 * @param {object} [config] user config (`v.display_config`)
 * @returns {{min:number, max:number, source:'user'|'a2l'|'default'}}
 */
export function numericRange(v, config) {
  const base = a2lRange(v)
  const cfgMin = finiteOrNull(config?.min)
  const cfgMax = finiteOrNull(config?.max)
  const fallback = base || DEFAULT_NUMERIC_RANGE
  const min = cfgMin ?? fallback.min
  const max = cfgMax ?? fallback.max
  const source = cfgMin !== null || cfgMax !== null ? 'user' : base ? 'a2l' : 'default'
  // A user can type min >= max; keep the widget sane instead of dividing by 0.
  if (max <= min) return { min, max: min + 1, source }
  return { min, max, source }
}

/** How much of the range a value covers, clamped to 0..1 (`null` = unknown). */
export function numericFraction(value, range) {
  const num = numericValue(value)
  if (num === null || !range) return null
  const span = range.max - range.min
  if (!(span > 0)) return null
  const fraction = (num - range.min) / span
  if (!Number.isFinite(fraction)) return null
  return Math.min(1, Math.max(0, fraction))
}

function clampSegments(value) {
  const num = finiteOrNull(value)
  if (num === null) return NUMERIC_DEFAULTS.segments
  return Math.min(20, Math.max(0, Math.round(num)))
}

function normalizeColor(value) {
  const text = String(value ?? '').trim()
  return /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(text) ? text : NUMERIC_DEFAULTS.color
}

/**
 * Fully resolved renderer config: defaults applied, range resolved, values
 * clamped. Renderers only ever see this shape, never the raw user object.
 */
export function resolveNumericConfig(v, config) {
  const range = numericRange(v, config)
  return normalizeConfig({ ...config, min: range.min, max: range.max, rangeSource: range.source })
}

/**
 * Clamp/validate a config that already carries its final bounds (what
 * `CardDisplay` hands to a renderer). Keeps renderers free of defaulting logic.
 */
export function normalizeConfig(config) {
  const min = finiteOrNull(config?.min)
  const max = finiteOrNull(config?.max)
  const range = min !== null && max !== null && max > min
    ? { min, max }
    : { ...DEFAULT_NUMERIC_RANGE }
  return {
    min: range.min,
    max: range.max,
    rangeSource: config?.rangeSource || 'user',
    orientation: config?.orientation === 'vertical' ? 'vertical' : NUMERIC_DEFAULTS.orientation,
    color: normalizeColor(config?.color),
    segments: clampSegments(config?.segments),
  }
}