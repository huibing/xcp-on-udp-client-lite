/**
 * Display-style registry.
 *
 * One table, keyed by VALUE_KIND, is the single place that knows which card
 * renderers exist. `Experiment.vue` never branches on a style id — it asks this
 * module for the renderer and mounts it with `<component :is>`.
 *
 * ── Adding a style for another kind later ────────────────────────────────────
 *   1. write a renderer component under `experiment/components/` that accepts
 *      `state`/`text`/`labels`/`editable`/`busy` (+ `value`/`config`/`unit` for
 *      numeric ones) and emits `select(value)` (see BooleanToggle.vue for the
 *      contract),
 *   2. import it here and add an entry to the matching kind's array,
 *   3. done — the card, the style picker and the experiment file all pick it up.
 *      Enum renderers (segmented control, dropdown) are the obvious next entries.
 *
 * Rules that keep the file format stable:
 *   - `id` is what gets written to the experiment JSON. Never rename an id that
 *     has shipped; add a new one and keep the old as an alias instead.
 *   - the first entry of each kind's array is the default, and its id must match
 *     DEFAULT_STYLE_ID.
 *   - `options` (optional) declares the tunable settings of a style. The options
 *     popup renders whatever is declared here, so a style that needs a new knob
 *     adds a descriptor instead of another dialog. Option keys are persisted in
 *     `v.display_config`.
 */

import { VALUE_KIND } from './kinds'
import { NUMERIC_DEFAULTS } from './numeric'
import BooleanText from '../components/BooleanText.vue'
import BooleanToggle from '../components/BooleanToggle.vue'
import BooleanRadio from '../components/BooleanRadio.vue'
import BooleanBulb from '../components/BooleanBulb.vue'
import NumericGauge from '../components/NumericGauge.vue'
import NumericBar from '../components/NumericBar.vue'

/**
 * @typedef {object} StyleOption
 * @property {string} key       key inside `display_config`
 * @property {string} label     row label in the options popup
 * @property {'number'|'select'|'color'} type
 * @property {number} [step]    number input step
 * @property {Array<{value:string,label:string}>} [choices] for `select`
 * @property {*} [default]      shown when the key is unset
 * @property {'min'|'max'} [auto] use the resolved range bound as the placeholder
 *
 * @typedef {object} DisplayStyle
 * @property {string} id      stable id persisted in the experiment file
 * @property {string} label   shown in the style picker
 * @property {string} icon    short glyph for the picker row
 * @property {object} component Vue component rendered inside the card
 * @property {boolean} interactive true when the control can write a value
 * @property {boolean} [numeric] renderer wants `value` / `config` / `unit`
 * @property {StyleOption[]} [options] tunable settings rendered by DisplayOptions.vue
 */

/** Range bounds are shared by every numeric style; empty means "use the A2L". */
const RANGE_OPTIONS = [
  { key: 'min', label: 'Range min', type: 'number', step: 'any', auto: 'min' },
  { key: 'max', label: 'Range max', type: 'number', step: 'any', auto: 'max' },
]

const GAUGE_OPTIONS = [
  ...RANGE_OPTIONS,
  { key: 'color', label: 'Arc color', type: 'color', default: NUMERIC_DEFAULTS.color },
]

const BAR_OPTIONS = [
  ...RANGE_OPTIONS,
  {
    key: 'orientation',
    label: 'Orientation',
    type: 'select',
    default: NUMERIC_DEFAULTS.orientation,
    choices: [
      { value: 'horizontal', label: 'Horizontal' },
      { value: 'vertical', label: 'Vertical' },
    ],
  },
  { key: 'color', label: 'Level color', type: 'color', default: NUMERIC_DEFAULTS.color },
  {
    key: 'segments',
    label: 'Cells',
    type: 'number',
    step: 1,
    placeholder: 'solid',
    default: NUMERIC_DEFAULTS.segments,
  },
]

export const DISPLAY_STYLES = Object.freeze({
  [VALUE_KIND.BOOLEAN]: [
    { id: 'text', label: 'Text', icon: '0/1', component: BooleanText, interactive: false },
    { id: 'toggle', label: 'Toggle', icon: '⇄', component: BooleanToggle, interactive: true },
    { id: 'radio', label: 'Radio', icon: '◉', component: BooleanRadio, interactive: true },
    { id: 'bulb', label: 'Bulb', icon: '💡', component: BooleanBulb, interactive: true },
  ],
  [VALUE_KIND.NUMERIC]: [
    { id: 'text', label: 'Text', icon: '123', component: BooleanText, interactive: false },
    {
      id: 'gauge',
      label: 'Gauge',
      icon: '◔',
      component: NumericGauge,
      interactive: false,
      numeric: true,
      options: GAUGE_OPTIONS,
    },
    {
      id: 'bar',
      label: 'Level bar',
      icon: '▭',
      component: NumericBar,
      interactive: false,
      numeric: true,
      options: BAR_OPTIONS,
    },
  ],
  // [VALUE_KIND.ENUM]: [ … ],
})

export const DEFAULT_STYLE_ID = Object.freeze({
  [VALUE_KIND.BOOLEAN]: 'text',
  [VALUE_KIND.NUMERIC]: 'text',
  [VALUE_KIND.ENUM]: 'text',
  [VALUE_KIND.MAP]: 'text',
  [VALUE_KIND.UNKNOWN]: 'text',
})

/** Every style offered for a kind (empty array when the kind has none yet). */
export function stylesFor(kind) {
  return DISPLAY_STYLES[kind] || []
}

/** True when the kind offers more than the single default rendering. */
export function hasStyleChoices(kind) {
  return stylesFor(kind).length > 1
}

export function defaultStyleFor(kind) {
  const styles = stylesFor(kind)
  if (styles.length > 0) return styles[0].id
  return DEFAULT_STYLE_ID[kind] || 'text'
}

/**
 * Resolve a stored style id to a style descriptor, falling back to the kind's
 * default when the id is missing (old experiment files) or unknown (a style
 * removed in a later build).
 */
export function resolveStyle(kind, styleId) {
  const styles = stylesFor(kind)
  if (styles.length === 0) {
    return { id: 'text', label: 'Text', icon: '', component: BooleanText, interactive: false }
  }
  return styles.find(s => s.id === styleId) || styles[0]
}

/** Tunable settings of a style (empty array when the style has none). */
export function optionsFor(kind, styleId) {
  return resolveStyle(kind, styleId).options || []
}

/** True when the style exposes an options popup (range, orientation, color …). */
export function hasOptions(kind, styleId) {
  return optionsFor(kind, styleId).length > 0
}

/** The id to persist for a variable: explicit choice, else the kind default. */
export function styleIdFor(variable, kind) {
  const stored = variable?.display_style
  const styles = stylesFor(kind)
  if (stored && styles.some(s => s.id === stored)) return stored
  return defaultStyleFor(kind)
}