import { expect, test } from 'claude-code/testing'

const STATUS = 'Workspace: E:\\proj\nFiles: 63  Symbols: 1654  Edges: 1598\n'

test('falls back to cargo bin when the CLI is not on PATH', async ($, on) => {
  const tried: string[] = []
  on('env.get', (_, e) => ({ value: e.name === 'USERPROFILE' ? 'C:/Users/me' : undefined }))
  on('process.run', (_, e) => {
    tried.push(e.argv[0] ?? '')
    return e.argv[0] === 'C:/Users/me/.cargo/bin/adaptive-codegraph.exe'
      ? { value: { exitCode: 0, stdout: STATUS, stderr: '' } }
      : { deny: 'ENOENT' }
  })
  on('ui.status', () => ({ value: undefined }))

  const r = await $.command.run({ command: 'codegraph-status' })
  expect(r.text).toBe(STATUS)
  expect(tried).toEqual([
    'adaptive-codegraph',
    'adaptive-codegraph.exe',
    'C:/Users/me/.cargo/bin/adaptive-codegraph.exe',
  ])
})

test('says why when the CLI cannot start anywhere', async ($, on) => {
  on('env.get', () => ({ value: undefined }))
  on('process.run', () => ({ deny: 'ENOENT' }))
  on('ui.status', () => ({ value: undefined }))

  const r = await $.command.run({ command: 'codegraph-status' })
  expect(r.text).toMatch(/Could not start adaptive-codegraph/)
  expect(r.text).toMatch(/Last error: adaptive-codegraph\.exe: .*ENOENT/)
})
