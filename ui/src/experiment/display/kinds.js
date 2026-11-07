/**
 * Value-kind resolution for experiment cards.
 *
 * A variable's *kind* (boolean / enum / numeric / map …) is DERIVED from its A2L
 * metadata every time it is rendered. It is deliberately never persisted in the
 * experiment file: the A2L can be regenerated between sessions, and a stored
 * kind would silently rot (a U8 that used to be 0..1 becoming 0..255, etc.).
 *
 * Only `boolean` is wired to alternative display styles today; the other kinds
 * exist so that adding a numeric or enum renderer later needs no changes here.
 */

export const VALUE_KIND = Object.freeze({
  BOOLEAN: 'boolean',
  ENUM: 'enum',
  NUMERIC: 'numeric',
  MAP: 'map',
  UNKNOWN: 'unknown',
})

/** Types the A2L reader can emit for a scalar (`type_name()` / `{:?}` of the enum). */
const BOOLEAN_TYPES = new Set(['BOOL', 'BOOLEAN', 'A_BOOLEAN'])

const INTEGER_TYPES = new Set([
  'U8', 'U16', 'U32', 'U64',
  'I8', 'I16', 'I32', 'I64',
  // A2L alias spellings some readers emit
  'UBYTE', 'SBYTE', 'UWORD', 'SWORD', 'ULONG', 'SLONG', 'A_UINT64', 'A_INT64',
])

const FLOAT_TYPES = new Set(['F32', 'F64', 'FLOAT32', 'FLOAT64', 'REAL32', 'REAL64'])

/** Characteristic types that describe an array/map rather than a scalar. */
const AGGREGATE_CHARACTERISTICS = new Set([
  'MAP', 'CURVE', 'VAL_BLK', 'CUBOID', 'ASCII', 'VALUE_RANGE',
])

function upper(value) {
  return String(value ?? '').trim().toUpperCase()
}

/** True when the A2L declares the variable as a boolean scalar. */
export function isBooleanType(type) {
  return BOOLEAN_TYPES.has(upper(type))
}

export function isIntegerType(type) {
  return INTEGER_TYPES.has(upper(type))
}

export function isNumericType(type) {
  return INTEGER_TYPES.has(upper(type)) || FLOAT_TYPES.has(upper(type))
}

/** `U8[4]` / `F32[16]` — an array type, never a single boolean. */
export function isArrayType(type) {
  return /\[/.test(String(type ?? ''))
}

function enumOptions(v) {
  return Array.isArray(v?.enum_options) ? v.enum_options : []
}

/** An enum whose two options are exactly raw 0 and 1 is a boolean in disguise. */
function isBinaryEnum(v) {
  if (!v?.is_enum) return false
  const opts = enumOptions(v)
  if (opts.length !== 2) return false
  const raws = opts.map(o => Number(o.raw)).sort((a, b) => a - b)
  return raws[0] === 0 && raws[1] === 1
}

/**
 * Decide whether a variable is a boolean.
 *
 * Three independent conventions appear in real A2L files, so all three are
 * accepted: an explicit BOOL data type, a 2-option enum on 0/1, or an integer
 * scalar whose declared range is exactly 0..1 (the most common case in
 * hand-written A2L — there is no BOOL keyword in many exports).
 */
export function isBooleanVar(v) {
  if (!v) return false
  if (isArrayType(v.type)) return false
  if (isBooleanType(v.type)) return true
  if (isBinaryEnum(v)) return true
  if (v.is_enum) return false
  if (!isIntegerType(v.type)) return false
  // `Number(null) === 0`, so guard the range explicitly.
  if (v.min == null || v.max == null) return false
  return Number(v.min) === 0 && Number(v.max) === 1
}

/**
 * Resolve the display kind of a variable.
 * @param {object} v variable object as produced by `experiment_variables`
 * @returns {string} one of VALUE_KIND
 */
export function resolveValueKind(v) {
  if (!v) return VALUE_KIND.UNKNOWN
  const characteristic = upper(v.characteristic_type || v.calType)
  if (isArrayType(v.type) || AGGREGATE_CHARACTERISTICS.has(characteristic)) {
    return VALUE_KIND.MAP
  }
  if (isBooleanVar(v)) return VALUE_KIND.BOOLEAN
  if (v.is_enum) return VALUE_KIND.ENUM
  if (isNumericType(v.type)) return VALUE_KIND.NUMERIC
  return VALUE_KIND.UNKNOWN
}