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

  // Refresh the status line after edits (the MCP server re-indexes on its
  // next query; this only reads the counts).
  on('tool.call', async ($, e, next) => {
    const ran = await next(e)
    if (e.tool === 'Edit' || e.tool === 'Write') {
      void readStatus($).then(s => s && $.ui.status(s.line))
    }
    return ran
  })
}
