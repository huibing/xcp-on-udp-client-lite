/**
 * Display-style registry.
 *
 * One table, keyed by VALUE_KIND, is the single place that knows which card
 * renderers exist. `Experiment.vue` never branches on a style id — it asks this
 * module for the renderer and mounts it with `<component :is>`.
 *
 * ── Adding a style for another kind later ────────────────────────────────────
 *   1. write a renderer component under `experiment/components/` that accepts
 *      `state`/`text`/`labels`/`editable`/`busy` and emits `select(value)`
 *      (see BooleanToggle.vue for the contract),
 *   2. import it here and add an entry to the matching kind's array,
 *   3. done — the card, the style picker and the experiment file all pick it up.
 *      Numeric renderers (gauge, bar, slider) and enum renderers (segmented
 *      control, dropdown) are the obvious next entries.
 *
 * Rules that keep the file format stable:
 *   - `id` is what gets written to the experiment JSON. Never rename an id that
 *     has shipped; add a new one and keep the old as an alias instead.
 *   - the first entry of each kind's array is the default, and its id must match
 *     DEFAULT_STYLE_ID.
 */

import { VALUE_KIND } from './kinds'
import BooleanText from '../components/BooleanText.vue'
import BooleanToggle from '../components/BooleanToggle.vue'
import BooleanRadio from '../components/BooleanRadio.vue'
import BooleanBulb from '../components/BooleanBulb.vue'

/**
 * @typedef {object} DisplayStyle
 * @property {string} id      stable id persisted in the experiment file
 * @property {string} label   shown in the style picker
 * @property {string} icon    short glyph for the picker row
 * @property {object} component Vue component rendered inside the card
 * @property {boolean} interactive true when the control can write a value
 */

export const DISPLAY_STYLES = Object.freeze({
  [VALUE_KIND.BOOLEAN]: [
    { id: 'text', label: 'Text', icon: '0/1', component: BooleanText, interactive: false },
    { id: 'toggle', label: 'Toggle', icon: '⇄', component: BooleanToggle, interactive: true },
    { id: 'radio', label: 'Radio', icon: '◉', component: BooleanRadio, interactive: true },
    { id: 'bulb', label: 'Bulb', icon: '💡', component: BooleanBulb, interactive: true },
  ],
  // [VALUE_KIND.NUMERIC]: [ … ],
  // [VALUE_KIND.ENUM]:    [ … ],
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

/** The id to persist for a variable: explicit choice, else the kind default. */
export function styleIdFor(variable, kind) {
  const stored = variable?.display_style
  const styles = stylesFor(kind)
  if (stored && styles.some(s => s.id === stored)) return stored
  return defaultStyleFor(kind)
}