import assert from 'node:assert/strict'
import { test } from 'node:test'
import { executeBounded, probeWorkflowRuntime, readRuntimeProfile } from '../workflow-runtime-probe.mjs'
import { runWorkflowRuntimeSmoke } from './workflow-runtime-smoke.mjs'

// Keep the .node.mjs suffix: Vitest must not import these node:test cases.
// Its worker can exit before executeBounded reaps the SIGTERM-ignoring child.

const DIGEST = `sha256:${'a'.repeat(64)}`
const PROFILE = { id: 'prepared-linux-runner', image: `example.invalid/praxis-worker@${DIGEST}` }

test('exported probes reject unbounded timeouts before spawning', async () => {
  for (const timeoutMs of [0, -1, Infinity, 30_001, 1.5]) {
    const report = await probeWorkflowRuntime({ profile: PROFILE, platform: 'linux', timeoutMs, executor: () => { throw new Error('must not run') } })
    assert.equal(report.status, 'capability_unavailable')
    await assert.rejects(executeBounded(process.execPath, [], { timeoutMs }), /timeoutMs/)
  }
})

test('bounded executor reaps a process that ignores termination', async () => {
  const result = await executeBounded(process.execPath, ['-e', "process.on('SIGTERM', () => {}); console.log(process.pid); setInterval(() => {}, 10)"], { timeoutMs: 250 })
  assert.equal(result.timedOut, true)
  const pid = Number(result.stdout.trim())
  assert.ok(pid > 0)
  assert.throws(() => process.kill(pid, 0), { code: 'ESRCH' })
})

test('bounded executor stops excessive output and reaps the process', async () => {
  const result = await executeBounded(process.execPath, ['-e', "process.stdout.write('x'.repeat(2*1024*1024)); setInterval(() => {}, 10)"], { timeoutMs: 3000 })
  assert.equal(result.errorCode, 'output_limit')
  assert.ok(Buffer.byteLength(result.stdout) <= 1024*1024)
})
const READY_INFO = JSON.stringify({
  host: {
    security: { rootless: true },
    cgroupVersion: 'v2',
    cgroupControllers: ['cpu', 'memory', 'pids'],
  },
})

function result(code, stdout = '') {
  return { code, stdout, stderr: '', timedOut: false }
}

function readyExecutor(calls) {
  return async (file, args) => {
    calls.push([file, args])
    if (args[0] === 'info') return result(0, READY_INFO)
    if (args[0] === 'image' && args[1] === 'inspect') return result(0, JSON.stringify([{ Digest: DIGEST }]))
    throw new Error(`unexpected invocation: ${file} ${args.join(' ')}`)
  }
}

function status(report, id) {
  return report.checks.find((item) => item.id === id)?.status
}

test('non-Linux hosts fail closed before calling Podman', async () => {
  let calls = 0
  const report = await probeWorkflowRuntime({
    profile: PROFILE,
    platform: 'darwin',
    executor: async () => { calls += 1; return result(0) },
  })
  assert.equal(report.status, 'capability_unavailable')
  assert.equal(status(report, 'host_linux'), 'fail')
  assert.equal(calls, 0)
  assert.equal(report.runtime_verified, false)
})

test('missing Podman reports the executable prerequisite without attempting image operations', async () => {
  const calls = []
  const report = await probeWorkflowRuntime({
    profile: PROFILE,
    platform: 'linux',
    executor: async (file, args) => {
      calls.push([file, args])
      return { code: null, stdout: '', stderr: '', errorCode: 'ENOENT' }
    },
  })
  assert.equal(report.status, 'capability_unavailable')
  assert.equal(status(report, 'podman_callable'), 'fail')
  assert.equal(status(report, 'pinned_image_present'), 'not_run')
  assert.deepEqual(calls, [['podman', ['info', '--format', 'json']]])
})

test('rootful Podman is rejected even when the local pinned image exists', async () => {
  const calls = []
  const report = await probeWorkflowRuntime({
    profile: PROFILE,
    platform: 'linux',
    executor: async (file, args) => {
      calls.push([file, args])
      if (args[0] === 'info') {
        return result(0, JSON.stringify({ host: { security: { rootless: false }, cgroupVersion: 'v2', cgroupControllers: ['cpu', 'memory', 'pids'] } }))
      }
      return result(0, JSON.stringify([{ Digest: DIGEST }]))
    },
  })
  assert.equal(report.status, 'capability_unavailable')
  assert.equal(status(report, 'podman_rootless'), 'fail')
  assert.equal(status(report, 'pinned_image_present'), 'pass')
  assert.equal(calls.length, 2)
})

test('malformed Podman info fails rootless and resource checks but remains bounded', async () => {
  const calls = []
  const report = await probeWorkflowRuntime({
    profile: PROFILE,
    platform: 'linux',
    executor: async (file, args) => {
      calls.push([file, args])
      return args[0] === 'info' ? result(0, '{bad json') : result(0, JSON.stringify([{ Digest: DIGEST }]))
    },
  })
  assert.equal(report.status, 'capability_unavailable')
  assert.equal(status(report, 'podman_rootless'), 'fail')
  assert.equal(status(report, 'cgroup_v2_resource_limits'), 'fail')
  assert.equal(status(report, 'pinned_image_present'), 'pass')
  assert.equal(calls.length, 2)
})

test('digest mismatch fails even if Podman returns an inspect record', async () => {
  const calls = []
  const report = await probeWorkflowRuntime({
    profile: PROFILE,
    platform: 'linux',
    executor: async (file, args) => {
      calls.push([file, args])
      return args[0] === 'info'
        ? result(0, READY_INFO)
        : result(0, JSON.stringify([{ Digest: `sha256:${'b'.repeat(64)}` }]))
    },
  })
  assert.equal(report.status, 'capability_unavailable')
  assert.equal(status(report, 'pinned_image_present'), 'fail')
  assert.match(report.missing_capabilities.find((item) => item.id === 'pinned_image_present').detail, /does not match/)
  assert.equal(calls.length, 2)
})

test('the preflight invokes only local read-only Podman commands', async () => {
  const calls = []
  const report = await probeWorkflowRuntime({ profile: PROFILE, platform: 'linux', executor: readyExecutor(calls) })
  assert.equal(report.status, 'prerequisite_ready')
  assert.deepEqual(calls, [
    ['podman', ['info', '--format', 'json']],
    ['podman', ['image', 'inspect', '--format', 'json', PROFILE.image]],
  ])
  const forbidden = /^(pull|run|create|start|login|build|push)$/
  assert.equal(calls.some(([, args]) => forbidden.test(args[0])), false)
})

test('an injected hanging command times out and reports unavailable capability', async () => {
  const report = await probeWorkflowRuntime({
    profile: PROFILE,
    platform: 'linux',
    timeoutMs: 10,
    executor: () => new Promise(() => {}),
  })
  assert.equal(report.status, 'capability_unavailable')
  assert.equal(status(report, 'podman_callable'), 'fail')
  assert.match(report.missing_capabilities[0].detail, /timed out/)
})

test('ready prerequisites are not reported as a verified runtime or vendor smoke', async () => {
  const calls = []
  const report = await probeWorkflowRuntime({ profile: PROFILE, platform: 'linux', executor: readyExecutor(calls) })
  assert.equal(report.status, 'prerequisite_ready')
  assert.equal(report.runtime_verified, false)
  assert.deepEqual(report.remaining_evidence, [
    'vendor_auth_and_attempt_proxy',
    'network_egress_enforcement',
    'sandbox_mount_isolation',
    'resource_limit_enforcement',
    'container_exit_and_child_cleanup',
    'host_database_access_denial',
  ])
})

test('smoke entry requires an explicit profile and fails closed without one', async () => {
  const report = await runWorkflowRuntimeSmoke([], { platform: 'linux' })
  assert.equal(report.status, 'capability_unavailable')
  assert.equal(report.runtime_verified, false)
  assert.equal(report.missing_capabilities[0].id, 'runtime_profile')
})

test('profile arguments support a supplied digest without inventing vendor credentials', async () => {
  const { profile, timeoutMs } = await readRuntimeProfile(['--image-digest', PROFILE.image, '--timeout-ms', '25'])
  assert.deepEqual(profile, { image: PROFILE.image })
  assert.equal(timeoutMs, 25)
})

test('smoke entry accepts an explicitly supplied profile and records only immutable digests', async () => {
  const calls = []
  const report = await runWorkflowRuntimeSmoke(['--profile', 'prepared-runtime.json'], {
    read: async () => JSON.stringify({ ...PROFILE, id: 'private-registry-description', cli_digest: DIGEST }),
    platform: 'linux',
    executor: readyExecutor(calls),
  })
  assert.equal(report.status, 'prerequisite_ready')
  assert.deepEqual(report.profile, { image_digest: DIGEST, cli_digest: DIGEST })
  assert.equal(JSON.stringify(report).includes('private-registry-description'), false)
})
