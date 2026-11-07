import type { EngineInterface, Register } from 'claude-code'

// The adaptive-codegraph CLI (installed by the project's install.sh).
const CLI = 'adaptive-codegraph'

type Status = { line: string; text: string }

// The status line already shows this plugin's name as its label, so the text
// is only a status symbol and the numbers.
const count = (n: number) =>
  n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`

/** Read the index status (never builds or refreshes the index). */
async function readStatus($: EngineInterface): Promise<Status | undefined> {
  try {
    const r = await $.process.run([CLI, 'status'], { timeoutMs: 30_000 })
    if (r.exitCode !== 0) return undefined
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
    return { text: s ? s.text : `${CLI} is not on PATH. Run install.sh from the adaptive-codegraph repo.` }
  })

  on('command.run', { command: 'codegraph-reindex' }, async $ => {
    try {
      const r = await $.process.run([CLI, 'index'], { timeoutMs: 600_000 })
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
