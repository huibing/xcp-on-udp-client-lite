/**
 * Safe math expression evaluator — no eval(), no Function().
 *
 * Supported operators and functions:
 *   Arithmetic : + − * / % ^ (power, right-associative)
 *   Comparison : == != < <= > >=
 *   Logical    : && || !  (returns 1 / 0)
 *   Trig       : sin cos tan asin acos atan atan2 sinh cosh tanh
 *   Math       : sqrt abs log log2 log10 exp pow
 *                ceil floor round sign hypot min max clamp
 *   Constants  : pi  e
 *   Variables  : any identifier not matching a built-in name
 */

// ── Built-in functions ────────────────────────────────────────────────────────

const FUNCTIONS = {
  sin:   (x)         => Math.sin(x),
  cos:   (x)         => Math.cos(x),
  tan:   (x)         => Math.tan(x),
  asin:  (x)         => Math.asin(x),
  acos:  (x)         => Math.acos(x),
  atan:  (x)         => Math.atan(x),
  atan2: (y, x)      => Math.atan2(y, x),
  sinh:  (x)         => Math.sinh(x),
  cosh:  (x)         => Math.cosh(x),
  tanh:  (x)         => Math.tanh(x),
  sqrt:  (x)         => Math.sqrt(x),
  abs:   (x)         => Math.abs(x),
  log:   (x)         => Math.log(x),
  log2:  (x)         => Math.log2(x),
  log10: (x)         => Math.log10(x),
  exp:   (x)         => Math.exp(x),
  pow:   (x, y)      => Math.pow(x, y),
  ceil:  (x)         => Math.ceil(x),
  floor: (x)         => Math.floor(x),
  round: (x)         => Math.round(x),
  sign:  (x)         => Math.sign(x),
  hypot: (...a)      => Math.hypot(...a),
  min:   (...a)      => Math.min(...a),
  max:   (...a)      => Math.max(...a),
  clamp: (x, lo, hi) => Math.min(hi, Math.max(lo, x)),
}

const CONSTANTS = {
  pi: Math.PI,
  PI: Math.PI,
  e:  Math.E,
  E:  Math.E,
}

// ── Tokenizer ─────────────────────────────────────────────────────────────────

const T = {
  NUM: 'NUM', IDENT: 'IDENT', COMMA: ',', LPAREN: '(', RPAREN: ')',
  PLUS: '+', MINUS: '-', STAR: '*', SLASH: '/', PERCENT: '%', CARET: '^',
  BANG: '!', EQ: '==', NEQ: '!=', LTE: '<=', GTE: '>=', LT: '<', GT: '>',
  AND: '&&', OR: '||', EOF: 'EOF',
}

function tokenize(expr) {
  const tokens = []
  let i = 0
  while (i < expr.length) {
    if (/\s/.test(expr[i])) { i++; continue }

    // Number literal (including scientific notation)
    if (/[0-9]/.test(expr[i]) || (expr[i] === '.' && /[0-9]/.test(expr[i + 1] ?? ''))) {
      let j = i
      while (j < expr.length && /[0-9.]/.test(expr[j])) j++
      if (j < expr.length && (expr[j] === 'e' || expr[j] === 'E')) {
        j++
        if (j < expr.length && (expr[j] === '+' || expr[j] === '-')) j++
        while (j < expr.length && /[0-9]/.test(expr[j])) j++
      }
      tokens.push({ type: T.NUM, value: parseFloat(expr.slice(i, j)) })
      i = j
      continue
    }

    // Identifier or keyword
    if (/[a-zA-Z_]/.test(expr[i])) {
      let j = i
      while (j < expr.length && /[a-zA-Z0-9_]/.test(expr[j])) j++
      tokens.push({ type: T.IDENT, value: expr.slice(i, j) })
      i = j
      continue
    }

    // Two-character operators (check before single-char)
    if (i + 1 < expr.length) {
      const two = expr.slice(i, i + 2)
      if (two === '==') { tokens.push({ type: T.EQ  }); i += 2; continue }
      if (two === '!=') { tokens.push({ type: T.NEQ }); i += 2; continue }
      if (two === '<=') { tokens.push({ type: T.LTE }); i += 2; continue }
      if (two === '>=') { tokens.push({ type: T.GTE }); i += 2; continue }
      if (two === '&&') { tokens.push({ type: T.AND }); i += 2; continue }
      if (two === '||') { tokens.push({ type: T.OR  }); i += 2; continue }
    }

    // Single-character tokens
    const ch = expr[i]
    switch (ch) {
      case '+': tokens.push({ type: T.PLUS    }); break
      case '-': tokens.push({ type: T.MINUS   }); break
      case '*': tokens.push({ type: T.STAR    }); break
      case '/': tokens.push({ type: T.SLASH   }); break
      case '%': tokens.push({ type: T.PERCENT }); break
      case '^': tokens.push({ type: T.CARET   }); break
      case '!': tokens.push({ type: T.BANG    }); break
      case '<': tokens.push({ type: T.LT      }); break
      case '>': tokens.push({ type: T.GT      }); break
      case '(': tokens.push({ type: T.LPAREN  }); break
      case ')': tokens.push({ type: T.RPAREN  }); break
      case ',': tokens.push({ type: T.COMMA   }); break
      default:  throw new Error(`Unexpected character '${ch}' at position ${i}`)
    }
    i++
  }
  tokens.push({ type: T.EOF })
  return tokens
}

// ── Recursive-descent parser → AST ───────────────────────────────────────────
//
// Grammar (precedence, lowest → highest):
//   expr    → or_expr
//   or      → and ('||' and)*
//   and     → cmp ('&&' cmp)*
//   cmp     → add (('==' | '!=' | '<' | '<=' | '>' | '>=') add)?
//   add     → mul (('+' | '-') mul)*
//   mul     → unary (('*' | '/' | '%') unary)*
//   unary   → '!' unary | '-' unary | '+' unary | power
//   power   → primary ('^' unary)?          ← right-associative
//   primary → NUMBER | IDENT '(' args ')' | IDENT | '(' expr ')'

class Parser {
  constructor(tokens) { this.tokens = tokens; this.pos = 0 }

  peek()       { return this.tokens[this.pos] }
  consume()    { return this.tokens[this.pos++] }
  expect(type) {
    const tok = this.consume()
    if (tok.type !== type) throw new Error(`Expected '${type}', got '${tok.type}'`)
    return tok
  }
  match(...types) { return types.includes(this.peek().type) ? this.consume() : null }

  parse() {
    const node = this.parseOr()
    if (this.peek().type !== T.EOF)
      throw new Error(`Unexpected token '${this.peek().type}' — check for mismatched parentheses`)
    return node
  }

  parseOr() {
    let left = this.parseAnd()
    while (this.peek().type === T.OR) {
      this.consume()
      left = { op: '||', left, right: this.parseAnd() }
    }
    return left
  }

  parseAnd() {
    let left = this.parseCmp()
    while (this.peek().type === T.AND) {
      this.consume()
      left = { op: '&&', left, right: this.parseCmp() }
    }
    return left
  }

  parseCmp() {
    const left = this.parseAdd()
    const op   = this.match(T.EQ, T.NEQ, T.LTE, T.GTE, T.LT, T.GT)
    if (op) return { op: op.type, left, right: this.parseAdd() }
    return left
  }

  parseAdd() {
    let left = this.parseMul()
    let op
    while ((op = this.match(T.PLUS, T.MINUS))) {
      left = { op: op.type, left, right: this.parseMul() }
    }
    return left
  }

  parseMul() {
    let left = this.parseUnary()
    let op
    while ((op = this.match(T.STAR, T.SLASH, T.PERCENT))) {
      left = { op: op.type, left, right: this.parseUnary() }
    }
    return left
  }

  parseUnary() {
    if (this.peek().type === T.BANG)  { this.consume(); return { op: 'u!', operand: this.parseUnary() } }
    if (this.peek().type === T.MINUS) { this.consume(); return { op: 'u-', operand: this.parseUnary() } }
    if (this.peek().type === T.PLUS)  { this.consume(); return this.parseUnary() }
    return this.parsePower()
  }

  parsePower() {
    const base = this.parsePrimary()
    if (this.peek().type === T.CARET) {
      this.consume()
      return { op: '^', left: base, right: this.parseUnary() }   // right-associative
    }
    return base
  }

  parsePrimary() {
    const tok = this.peek()

    if (tok.type === T.NUM)   { this.consume(); return { type: 'num', value: tok.value } }

    if (tok.type === T.IDENT) {
      this.consume()
      if (this.peek().type === T.LPAREN) {
        this.consume()
        const args = []
        if (this.peek().type !== T.RPAREN) {
          args.push(this.parseOr())
          while (this.match(T.COMMA)) args.push(this.parseOr())
        }
        this.expect(T.RPAREN)
        return { type: 'call', name: tok.value, args }
      }
      return { type: 'var', name: tok.value }
    }

    if (tok.type === T.LPAREN) {
      this.consume()
      const inner = this.parseOr()
      this.expect(T.RPAREN)
      return inner
    }

    throw new Error(
      tok.type === T.EOF
        ? 'Unexpected end of expression'
        : `Unexpected '${tok.type}' token`
    )
  }
}

// ── AST evaluator ─────────────────────────────────────────────────────────────

function evalNode(node, vars) {
  switch (node.type) {
    case 'num': return node.value
    case 'var': {
      const n = node.name
      if (n in CONSTANTS) return CONSTANTS[n]
      if (n in vars)      return vars[n]
      throw new Error(`Unknown variable '${n}' — add it in the Variables table`)
    }
    case 'call': {
      const fn = FUNCTIONS[node.name]
      if (!fn) throw new Error(`Unknown function '${node.name}'`)
      return fn(...node.args.map(a => evalNode(a, vars)))
    }
  }

  switch (node.op) {
    case '+':  return evalNode(node.left, vars) + evalNode(node.right, vars)
    case '-':  return evalNode(node.left, vars) - evalNode(node.right, vars)
    case '*':  return evalNode(node.left, vars) * evalNode(node.right, vars)
    case '/':  return evalNode(node.left, vars) / evalNode(node.right, vars)
    case '%':  return evalNode(node.left, vars) % evalNode(node.right, vars)
    case '^':  return Math.pow(evalNode(node.left, vars), evalNode(node.right, vars))

    case '==': return evalNode(node.left, vars) === evalNode(node.right, vars) ? 1 : 0
    case '!=': return evalNode(node.left, vars) !== evalNode(node.right, vars) ? 1 : 0
    case '<':  return evalNode(node.left, vars)  <  evalNode(node.right, vars) ? 1 : 0
    case '<=': return evalNode(node.left, vars)  <= evalNode(node.right, vars) ? 1 : 0
    case '>':  return evalNode(node.left, vars)  >  evalNode(node.right, vars) ? 1 : 0
    case '>=': return evalNode(node.left, vars)  >= evalNode(node.right, vars) ? 1 : 0

    case '&&': {
      const l = evalNode(node.left, vars)
      return l ? (evalNode(node.right, vars) ? 1 : 0) : 0
    }
    case '||': {
      const l = evalNode(node.left, vars)
      return l ? 1 : (evalNode(node.right, vars) ? 1 : 0)
    }
    case 'u!': return evalNode(node.operand, vars) ? 0 : 1
    case 'u-': return -evalNode(node.operand, vars)
  }
  throw new Error(`Internal error: unknown op '${node.op}'`)
}

// ── Variable collection (walk AST) ───────────────────────────────────────────

function collectVarNames(node, out = new Set()) {
  if (!node) return out
  if (node.type === 'var' && !(node.name in CONSTANTS) && !(node.name in FUNCTIONS))
    out.add(node.name)
  if (node.type === 'call') node.args.forEach(a => collectVarNames(a, out))
  collectVarNames(node.left,    out)
  collectVarNames(node.right,   out)
  collectVarNames(node.operand, out)
  return out
}

// ── Public API ────────────────────────────────────────────────────────────────

/**
 * Compile an expression string into a reusable evaluator.
 *
 * @param {string} expression
 * @returns {{ evaluate: (vars: Record<string, number>) => number, variables: string[] }}
 * @throws on syntax error
 */
export function compile(expression) {
  const tokens    = tokenize(expression.trim())
  const ast       = new Parser(tokens).parse()
  const variables = [...collectVarNames(ast)]
  return {
    evaluate:  (vars) => evalNode(ast, vars),
    variables,
  }
}

/**
 * Linear interpolation: value of the signal at time t.
 * `times` must be sorted ascending. Uses binary search — O(log n).
 *
 * @param {Float64Array|number[]} times
 * @param {Float64Array|number[]} values
 * @param {number} t
 * @returns {number}
 */
export function interpolate(times, values, t) {
  const n = times.length
  if (n === 0)           return NaN
  if (t <= times[0])     return values[0]
  if (t >= times[n - 1]) return values[n - 1]

  let lo = 0, hi = n - 1
  while (lo + 1 < hi) {
    const mid = (lo + hi) >>> 1
    if (times[mid] <= t) lo = mid
    else                 hi = mid
  }
  const frac = (t - times[lo]) / (times[hi] - times[lo])
  return values[lo] + frac * (values[hi] - values[lo])
}

/**
 * Merge multiple time arrays into one sorted, deduplicated Float64Array.
 *
 * @param {Array<Float64Array|number[]>} timesArray
 * @returns {Float64Array}
 */
export function mergeTimeBases(timesArray) {
  if (timesArray.length === 0) return new Float64Array(0)
  if (timesArray.length === 1) return Float64Array.from(timesArray[0])

  // Concatenate, sort, deduplicate
  let total = 0
  for (const t of timesArray) total += t.length
  const flat = new Float64Array(total)
  let pos = 0
  for (const t of timesArray) flat.set(t, pos), (pos += t.length)
  flat.sort()

  const out = [flat[0]]
  for (let i = 1; i < flat.length; i++) {
    if (flat[i] !== flat[i - 1]) out.push(flat[i])
  }
  return new Float64Array(out)
}

/**
 * Compute a derived signal from an expression and source signals.
 *
 * The output time base is the sorted union of all source signals' time bases.
 * Source signals are linearly interpolated at each output time point.
 *
 * @param {string} expression - Formula, e.g. "sin(a) * 2 + b"
 * @param {Record<string, { times: Float64Array, values: Float64Array }>} signalData
 *        Keys must match the variable names used in the expression.
 * @returns {{ times: Float64Array, values: Float64Array }}
 * @throws on syntax error or unknown variable/function
 */
export function computeDerivedSignal(expression, signalData) {
  const { evaluate } = compile(expression)

  const allTimes = Object.values(signalData).map(d => d.times)
  const times    = mergeTimeBases(allTimes)
  const values   = new Float64Array(times.length)
  const entries  = Object.entries(signalData)

  for (let i = 0; i < times.length; i++) {
    const t    = times[i]
    const vars = {}
    for (const [name, data] of entries) {
      vars[name] = interpolate(data.times, data.values, t)
    }
    values[i] = evaluate(vars)
  }

  return { times, values }
}
