import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { gzipSync } from 'node:zlib'
import { generateRunnerManifest } from '../generate-runner-manifest.mjs'

async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), 'praxis-manifest-'))
  t.after(() => rm(dir, { recursive: true, force: true }))
  const archive = join(dir, 'praxis-runner_0.1.0_linux_x86_64.tar.gz')
  const bytes = gzipSync('deterministic test fixture')
  await writeFile(archive, bytes)
  return { archive, bytes, version: '0.1.0', repository: 'JaehaSS/praxis', output: join(dir, 'manifest.json') }
}

test('pins exact artifact bytes and immutable version URL', async (t) => {
  const input = await fixture(t)
  const manifest = await generateRunnerManifest(input)
  assert.deepEqual(JSON.parse(await readFile(input.output, 'utf8')), manifest)
  assert.equal(manifest.releases[0].sha256, createHash('sha256').update(input.bytes).digest('hex'))
  assert.equal(manifest.releases[0].url, 'https://github.com/JaehaSS/praxis/releases/download/v0.1.0/praxis-runner_0.1.0_linux_x86_64.tar.gz')
  assert.equal(manifest.releases[0].architecture, 'x86_64')
})

test('rejects mismatched version or injected repository without overwriting manifest', async (t) => {
  const input = await fixture(t)
  await writeFile(input.output, 'keep existing')
  await assert.rejects(generateRunnerManifest({ ...input, version: '0.2.0' }), /filename/)
  await assert.rejects(generateRunnerManifest({ ...input, version: '../latest' }), /version/)
  await assert.rejects(generateRunnerManifest({ ...input, repository: 'owner/repo/../../other' }), /repository/)
  await assert.rejects(generateRunnerManifest({ ...input, repository: 'owner/..' }), /repository/)
  assert.equal(await readFile(input.output, 'utf8'), 'keep existing')
})

test('rejects non-gzip data and empty archives', async (t) => {
  const input = await fixture(t)
  await writeFile(input.archive, 'not an archive')
  await assert.rejects(generateRunnerManifest(input), /gzip/)
  await writeFile(input.archive, '')
  await assert.rejects(generateRunnerManifest(input), /regular file/)
})

test('rejects symlink inputs', { skip: process.platform === 'win32' }, async (t) => {
  const input = await fixture(t)
  const target = join(dirname(input.archive), 'actual.tar.gz')
  await writeFile(target, input.bytes)
  await rm(input.archive)
  await symlink(target, input.archive)
  await assert.rejects(generateRunnerManifest(input), /regular file/)
})
