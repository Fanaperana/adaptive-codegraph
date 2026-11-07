import type { EngineInterface, Register } from 'claude-code'

// The adaptive-codegraph CLI (installed by the project's install.sh).
const CLI = 'adaptive-codegraph'

type Status = { line: string; text: string }

// The status line already shows this plugin's name as its label, so the text
// is only a status symbol and the numbers.
const count = (n: number) =>
  n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`

// The command that last started, and why the last attempt failed.
let found: string | undefined
let lastError = ''

/**
 * Where the CLI may be: on PATH (with and without `.exe`, since no shell
 * resolves it), then cargo's bin folder, which a GUI app's PATH can miss.
 */
async function candidates($: EngineInterface): Promise<string[]> {
  const list = [CLI, `${CLI}.exe`]
  const cargo = await $.env.get('CARGO_HOME')
  const profile = await $.env.get('USERPROFILE')
  const home = await $.env.get('HOME')
  for (const dir of [cargo, profile && `${profile}/.cargo`, home && `${home}/.cargo`]) {
    if (dir) list.push(`${dir}/bin/${CLI}.exe`, `${dir}/bin/${CLI}`)
  }
  return [...new Set(list)]
}

/** Run the CLI with these arguments, trying each place it may be installed. */
async function runCli($: EngineInterface, args: string[], timeoutMs: number) {
  const tries = found ? [found, ...(await candidates($)).filter(c => c !== found)] : await candidates($)
  for (const cmd of tries) {
    try {
      const r = await $.process.run([cmd, ...args], { timeoutMs })
      found = cmd
      return r
    } catch (err) {
      lastError = `${cmd}: ${String(err)}`
    }
  }
  found = undefined
  throw new Error(lastError)
}

/** Read the index status (never builds or refreshes the index). */
async function readStatus($: EngineInterface): Promise<Status | undefined> {
  try {
    const r = await runCli($, ['status'], 30_000)
    if (r.exitCode !== 0) {
      lastError = (r.stderr || r.stdout).trim()
      return { line: '✖ status failed', text: `adaptive-codegraph status failed:\n${lastError}` }
    }
    if (r.stdout.startsWith('No index')) {
      return { line: '◌ not indexed', text: r.stdout }
    }
    const m = /Files: (\d+)\s+Symbols: (\d+)\s+Edges: (\d+)/.exec(r.stdout)
    const line = m ? `● ${count(Number(m[1]))} files · ${count(Number(m[2]))} symbols` : '● ready'
    return { line, text: r.stdout }
  } catch {
    return undefined
  }
}

const MISSING = '✖ CLI not found'

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'codegraph-status',
      description: 'Show what the code graph has indexed',
    })
    await $.command.register({
      name: 'codegraph-reindex',
      description: 'Rebuild the code graph index from scratch',
    })
    // The session-start hook may still be building the index: check again
    // shortly after.
    void readStatus($).then(s => $.ui.status(s ? s.line : MISSING))
    void $.clock.sleep(20_000).then(() => readStatus($)).then(s => s && $.ui.status(s.line))
    return next(e)
  })

  on('command.run', { command: 'codegraph-status' }, async $ => {
    const s = await readStatus($)
    $.ui.status(s ? s.line : MISSING)
    return {
      text: s
        ? s.text
        : `Could not start ${CLI} (not on PATH or in ~/.cargo/bin). Run install.sh from the adaptive-codegraph repo.\nLast error: ${lastError}`,
    }
  })

  on('command.run', { command: 'codegraph-reindex' }, async $ => {
    try {
      const r = await runCli($, ['index'], 600_000)
      const s = await readStatus($)
      $.ui.status(s ? s.line : MISSING)
      return { text: (r.stderr + r.stdout).trim() || 'Re-indexed.' }
    } catch (err) {
      return { text: `Re-index failed: ${String(err)}` }
    }
  })

  // Graph tool calls: show which tool is running and on what, then how it
  // went, then fall back to the index counts.
  let running = 0
  let latest = 0

  on('tool.call', async ($, e, next) => {
    const tool = graphTool(e.tool)
    if (!tool) {
      const ran = await next(e)
      // Refresh the counts after edits (the MCP server re-indexes on its
      // next query; this only reads the counts).
      if (e.tool === 'Edit' || e.tool === 'Write') {
        void readStatus($).then(s => s && running === 0 && $.ui.status(s.line))
      }
      return ran
    }

    const what = target(e as Record<string, unknown>)
    const label = what ? `${tool} · ${what}` : tool
    const call = ++latest
    running++
    $.ui.status(running > 1 ? `◐ ${running} graph calls` : `◐ ${label}`)
    const started = Date.now()
    try {
      const ran = await next(e)
      const secs = ((Date.now() - started) / 1000).toFixed(1)
      const failed = ran.deny !== undefined || ran.isError === true
      if (call === latest) {
        $.ui.status(failed ? `✖ ${label}` : `✓ ${label} · ${summary(ran.text)} · ${secs}s`)
      }
      return ran
    } finally {
      running--
      void $.clock.sleep(10_000).then(async () => {
        if (call !== latest || running > 0) return
        const s = await readStatus($)
        if (call === latest && running === 0) $.ui.status(s ? s.line : MISSING)
      })
    }
  }).catch(($, e, next) => next(e)) // a status bug must never block the tool

  // A badge on each graph tool's row in the transcript, above the engine's
  // own row: blue when done, yellow while running, red on failure.
  on('ui.render', { component: 'ToolUse' }, async ($, e, next) => {
    const own = await next(e)
    const tool = graphTool(e.props.tool)
    if (!tool) return own

    const { Box, Text } = $.ui.resolve(e)
    const { isRunning, isErrored, isInterrupted } = e.props
    const [color, state] = isErrored || isInterrupted
      ? ['error', isInterrupted ? 'interrupted' : 'failed']
      : isRunning
        ? ['warning', 'running…']
        : ['suggestion', summary(outputText(e.props.output))]
    const what = target((e.props.input ?? {}) as Record<string, unknown>)

    return (
      <Box flexDirection="column">
        <Box flexDirection="row">
          <Text backgroundColor={color} color="inverseText" bold>
            {' ◆ codegraph '}
          </Text>
          <Text color={color} bold>{` ${tool}`}</Text>
          <Text dimColor>{`${what ? ` · ${what}` : ''} · ${state}`}</Text>
        </Box>
        {own}
      </Box>
    )
  }).catch(($, e, next) => next(e)) // a badge bug falls back to the plain row
}

/** The text of an MCP result as stored for the row (a string or content blocks). */
function outputText(output: unknown): string {
  if (typeof output === 'string') return output
  const blocks = Array.isArray(output)
    ? output
    : output && typeof output === 'object' && Array.isArray((output as { content?: unknown }).content)
      ? (output as { content: unknown[] }).content
      : []
  return blocks
    .map(b => (b && typeof b === 'object' && typeof (b as { text?: unknown }).text === 'string' ? (b as { text: string }).text : ''))
    .join('\n')
}

/** `explain` for `mcp__plugin_adaptive-codegraph_adaptive-codegraph__explain`. */
function graphTool(name: string): string | undefined {
  const m = /^mcp__.*adaptive-codegraph__([a-z_]+)$/.exec(name)
  return m ? m[1] : undefined
}

const clip = (s: string, n = 40) => (s.length > n ? `${s.slice(0, n - 1)}…` : s)

/** The tool's main argument: the symbol, query, task or file it was asked about. */
function target(args: Record<string, unknown>): string {
  const str = (k: string) => (typeof args[k] === 'string' ? (args[k] as string).trim() : '')
  if (str('from') && str('to')) return clip(`${str('from')} → ${str('to')}`)
  for (const k of ['symbol', 'query', 'task', 'file', 'path']) {
    if (str(k)) return clip(str(k).replace(/\s+/g, ' '))
  }
  return ''
}

/** How much the tool returned, in lines. */
function summary(text: string | undefined): string {
  const lines = (text ?? '').split('\n').filter(l => l.trim()).length
  return `${lines} line${lines === 1 ? '' : 's'}`
}
