import { expect, test } from 'claude-code/testing'

const GRAPH = 'mcp__plugin_adaptive-codegraph_adaptive-codegraph__impact'

const row = (tool: string, props: Record<string, unknown> = {}) =>
  ({
    plugin: 'codegraph-status',
    component: 'ToolUse',
    requestId: 'toolu_1',
    props: {
      tool_use_id: 'toolu_1',
      tool,
      input: { symbol: 'Store::save', depth: 4 },
      isRunning: false,
      isErrored: false,
      isInterrupted: false,
      output: [{ type: 'text', text: 'line 1\nline 2\nline 3' }],
      ...props,
    },
  }) as const

test('a graph tool row gets the badge on every surface', async ($, on) => {
  // Stands for the engine's own row.
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)
    return <Text>engine row</Text>
  })

  for (const surface of ['terminal', 'desktop', 'vscode', 'mobile'] as const) {
    const ui = await $.ui.mount({ ...row(GRAPH), surface })
    expect((await ui.find({ type: 'Text', text: /codegraph/ }))?.text).toBe(' ◆ codegraph ')
    expect((await ui.find({ type: 'Text', text: /Store::save/ }))?.text).toBe(' · Store::save · 3 lines')
    expect((await ui.find({ type: 'Text', text: /engine row/ }))?.text).toBe('engine row')
    await ui.unmount()
  }
})

test('a running call says so', async ($, on) => {
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)
    return <Text>engine row</Text>
  })
  const ui = await $.ui.mount({ ...row(GRAPH, { isRunning: true, output: undefined }), surface: 'desktop' })
  expect((await ui.find({ type: 'Text', text: /running/ }))?.text).toBe(' · Store::save · running…')
  await ui.unmount()
})

test('other tools keep the plain row', async ($, on) => {
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)
    return <Text>engine row</Text>
  })
  const ui = await $.ui.mount({ ...row('Bash', { input: { command: 'ls' } }), surface: 'desktop' })
  expect(await ui.find({ type: 'Text', text: /codegraph/ })).toBe(undefined)
  expect((await ui.find({ type: 'Text', text: /engine row/ }))?.text).toBe('engine row')
  await ui.unmount()
})
