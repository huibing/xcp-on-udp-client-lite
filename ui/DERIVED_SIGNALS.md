# Derived Signals — Developer Guide

Derived signals are user-defined calculated channels built from one or more loaded MF4 signals using a mathematical expression. This document describes the architecture, the current set of supported operations, and how to extend the evaluator with new algorithms.

---

## Architecture Overview

```
DerivedSignalDialog.vue          ← UI: variable binding + formula input
        │
        │ calls
        ▼
ui/src/utils/expressionEvaluator.js   ← core evaluator (no eval/Function)
        │
        │ returns { times: Float64Array, values: Float64Array }
        ▼
Mf4Viewer.vue                    ← stores result in historyStore + chartGroups
        │
        │ persists via
        ▼
src-tauri/src/commands/mf4_viewer.rs  ← SignalConfig.derived (serde JSON)
```

**Key invariant:** the evaluator is a pure JavaScript function — no `eval()`, no `new Function()`. All computation is safe and sandboxed.

---

## Expression Evaluator (`expressionEvaluator.js`)

### Public API

```js
import { compile, interpolate, mergeTimeBases, computeDerivedSignal }
  from '@/utils/expressionEvaluator.js'
```

| Export | Signature | Purpose |
|--------|-----------|---------|
| `compile` | `(expr: string) → { evaluate, variables }` | Parse + compile expression; returns a reusable evaluator and the list of variable names used |
| `interpolate` | `(times, values, t) → number` | Linear interpolation at time `t` (binary search, O(log n)) |
| `mergeTimeBases` | `(timesArray[]) → Float64Array` | Sorted union of multiple time axes |
| `computeDerivedSignal` | `(expr, signalData) → { times, values }` | End-to-end computation of one derived signal |

### Pipeline

1. **Merge time bases** — sorted union of all source signals' time arrays (deduplication included)
2. **Interpolate** — for each merged time point, linearly interpolate every source signal
3. **Evaluate** — call the compiled expression evaluator with the interpolated values

---

## Currently Supported Operations

### Arithmetic
| Syntax | Meaning |
|--------|---------|
| `a + b` | Addition |
| `a - b` | Subtraction |
| `a * b` | Multiplication |
| `a / b` | Division |
| `a % b` | Modulo (remainder) |
| `a ^ b` | Power (right-associative: `2^3^2` = `2^(3^2)`) |
| `-a` | Unary negation |
| `+a` | Unary plus (no-op) |

### Comparison (return 1 or 0)
| Syntax | Meaning |
|--------|---------|
| `a == b` | Equal |
| `a != b` | Not equal |
| `a < b` | Less than |
| `a <= b` | Less than or equal |
| `a > b` | Greater than |
| `a >= b` | Greater than or equal |

### Logical (short-circuit, return 1 or 0)
| Syntax | Meaning |
|--------|---------|
| `a && b` | Logical AND |
| `a \|\| b` | Logical OR |
| `!a` | Logical NOT |

### Trigonometry
| Function | Description |
|----------|-------------|
| `sin(x)` | Sine (radians) |
| `cos(x)` | Cosine (radians) |
| `tan(x)` | Tangent (radians) |
| `asin(x)` | Arc sine |
| `acos(x)` | Arc cosine |
| `atan(x)` | Arc tangent |
| `atan2(y, x)` | Two-argument arc tangent |
| `sinh(x)` | Hyperbolic sine |
| `cosh(x)` | Hyperbolic cosine |
| `tanh(x)` | Hyperbolic tanh |

### Math
| Function | Description |
|----------|-------------|
| `sqrt(x)` | Square root |
| `abs(x)` | Absolute value |
| `log(x)` | Natural logarithm |
| `log2(x)` | Base-2 logarithm |
| `log10(x)` | Base-10 logarithm |
| `exp(x)` | e^x |
| `pow(x, y)` | x^y (same as `^` operator) |
| `ceil(x)` | Round up |
| `floor(x)` | Round down |
| `round(x)` | Round to nearest integer |
| `sign(x)` | Sign: −1, 0, or 1 |
| `hypot(x, y, …)` | Euclidean norm |
| `min(x, y, …)` | Minimum of arguments |
| `max(x, y, …)` | Maximum of arguments |
| `clamp(x, lo, hi)` | Clamp x to [lo, hi] |

### Per-Variable Transforms

Before a variable is fed into the formula, an optional **transform** can be applied to it per variable in the dialog's binding row.

| Transform | Label | Method | Notes |
|-----------|-------|--------|-------|
| None | `raw` | — | Values used as-is |
| Differentiation | `d/dt` | Central differences (forward/backward at endpoints) | Units: signal_unit / time_unit |
| Integration | `∫ dt` | Trapezoidal rule, cumulative from t=0 | Units: signal_unit × time_unit |

Implementations exported from `expressionEvaluator.js`:
```js
export function differentiate(times, values)  // → Float64Array
export function integrate(times, values)       // → Float64Array
```

These operate on the **full time series** before interpolation or formula evaluation, so non-uniform sample rates are handled correctly.

**Typical use cases:** `d/dt` position → velocity; `∫ dt` velocity → displacement; `∫ dt` power → energy.

### Constants
| Name | Value |
|------|-------|
| `pi` / `PI` | π ≈ 3.14159… |
| `e` / `E` | Euler's number ≈ 2.71828… |

---

## How to Add a New Built-in Function

All built-in functions are declared in the `FUNCTIONS` object near the top of `expressionEvaluator.js`.

**Step 1 — Add the function:**

```js
// expressionEvaluator.js
const FUNCTIONS = {
  // … existing functions …

  // Example: degrees ↔ radians converters
  deg2rad: (x) => x * (Math.PI / 180),
  rad2deg: (x) => x * (180 / Math.PI),

  // Example: moving-average (note: receives per-sample scalar — not a window fn)
  // For windowed/stateful algorithms see "Stateful Algorithms" below.
}
```

**Step 2 — Document it** in the Reference section of `DerivedSignalDialog.vue` (the collapsible "Reference" panel in the UI). Find the `<details>` block and add a row to the relevant table.

**Step 3 — Add a test** (optional but encouraged):
```js
// In a test file or browser console:
import { compile } from './expressionEvaluator.js'
const { evaluate } = compile('deg2rad(180)')
console.assert(Math.abs(evaluate({}) - Math.PI) < 1e-10)
```

That's it — no build step, no registration elsewhere. The parser handles all identifiers followed by `(` as function calls automatically.

---

## How to Add a New Constant

```js
// expressionEvaluator.js
const CONSTANTS = {
  pi: Math.PI,
  PI: Math.PI,
  e:  Math.E,
  E:  Math.E,

  // Add here:
  g:    9.80665,    // standard gravity m/s²
  c:    299792458,  // speed of light m/s
}
```

Constants take priority over variable names of the same spelling, so choose names that won't collide with common user variable names (`a`, `b`, `x`, etc.).

---

## Stateful / Windowed Algorithms

**d/dt differentiation and ∫ integration are now built-in** — select them from the transform dropdown in each variable binding row. For all other stateful operations:

Instead of adding them to `FUNCTIONS` (which only receive scalars), implement them as **post-processors** that operate on the full `{ times, values }` result or directly on source signals.

**Option A — New utility exported from `expressionEvaluator.js`:**

```js
/**
 * Compute a centered moving average of `values` over a time window of `windowSec` seconds.
 * @param {Float64Array} times
 * @param {Float64Array} values
 * @param {number} windowSec
 * @returns {Float64Array}
 */
export function movingAverage(times, values, windowSec) {
  const out = new Float64Array(values.length)
  const half = windowSec / 2
  for (let i = 0; i < times.length; i++) {
    const t0 = times[i] - half
    const t1 = times[i] + half
    let sum = 0, count = 0
    for (let j = i; j >= 0 && times[j] >= t0; j--)  { sum += values[j]; count++ }
    for (let j = i + 1; j < times.length && times[j] <= t1; j++) { sum += values[j]; count++ }
    out[i] = count ? sum / count : values[i]
  }
  return out
}
```

**Option B — Dedicated derived signal algorithm** added to `DerivedSignalDialog.vue` as a named "template" button that bypasses the expression field and computes results directly.

---

## Grammar Reference

The expression grammar (lowest → highest precedence):

```
expr    → or_expr
or      → and ('||' and)*
and     → cmp ('&&' cmp)*
cmp     → add ( ('==' | '!=' | '<' | '<=' | '>' | '>=') add )?
add     → mul ( ('+' | '-') mul )*
mul     → unary ( ('*' | '/' | '%') unary )*
unary   → '!' unary | '-' unary | '+' unary | power
power   → primary ( '^' unary )?        ← right-associative
primary → NUMBER | IDENT '(' args ')' | IDENT | '(' expr ')'
args    → expr (',' expr)*
```

Numbers support decimal point and scientific notation (`1.5e-3`).

---

## Rust Config Persistence

Derived signals are saved inside the viewer config file (JSON) via `SignalConfig.derived`:

```json
{
  "file_path": "",
  "channel": "my_derived",
  "color": "#00ff00",
  "derived": {
    "expression": "sin(a) * 2 + b",
    "variables": [
      { "var_name": "a", "signal_key": "/path/to/file.mf4::EngineSpeed", "transform": "diff" },
      { "var_name": "b", "signal_key": "/path/to/file.mf4::Throttle" }
    ]
  }
}
```

`transform` is omitted when `"none"` (raw). Supported values: `"diff"` (d/dt), `"integ"` (∫ dt).

The Rust structs are in `src-tauri/src/commands/mf4_viewer.rs`:
- `DerivedSignalMeta { expression: String, variables: Vec<DerivedVarBinding> }`
- `DerivedVarBinding { var_name: String, signal_key: String }`
- `SignalConfig.derived: Option<DerivedSignalMeta>` (`#[serde(skip_serializing_if = "Option::is_none")]`)

No Rust changes are needed when adding new functions or constants — they are purely frontend.

---

## Example Formulas

```
# Unit conversion
rpm_to_rads = a * pi / 30

# Norm of two signals
speed_mag = sqrt(vx^2 + vy^2)

# Conditional / gate
gated = a * (b > 0.5)

# Engineering formula
power_kw = torque * rpm * pi / 30 / 1000

# Angle in degrees
angle_deg = atan2(y, x) * 180 / pi
```
