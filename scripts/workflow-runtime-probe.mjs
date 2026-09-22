/**
 * Bounded, read-only prerequisite check for the Linux workflow adapter.
 *
 * This deliberately does not create containers, pull images, contact a
 * registry, read credentials, or attempt a vendor request.  A ready result
 * only means that the host has the prerequisites for a later runtime smoke.
 */
import { spawn } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { pathToFileURL } from 'node:url'

const DEFAULT_TIMEOUT_MS = 3_000
const REQUIRED_CGROUP_CONTROLLERS = ['cpu', 'memory', 'pids']
const PINNED_IMAGE = /^(?<reference>[^\s@]+)@(?<digest>sha256:[a-f0-9]{64})$/

function validTimeout(value) {
  return Number.isInteger(value) && value >= 1 && value <= 30_000
}

function check(id, status, detail) {
  return { id, status, detail }
}

function unavailable(id, detail) {
  return check(id, 'not_run', detail)
}

function capabilityUnavailable(profile, checks, missingCapabilities) {
  return {
    schema_version: 1,
    status: 'capability_unavailable',
    runtime: 'podman',
    profile: receiptProfile(profile),
    checks,
    missing_capabilities: missingCapabilities,
    runtime_verified: false,
    remaining_evidence: remainingEvidence(),
  }
}

// Profiles can be stored outside this script and may contain descriptive IDs
// or private registry references. Receipts retain only immutable digests.
function receiptProfile(profile) {
  if (!profile) return null
  return {
    image_digest: profile.image_digest,
    ...(profile.cli_digest ? { cli_digest: profile.cli_digest } : {}),
  }
}

function remainingEvidence() {
  return [
    'vendor_auth_and_attempt_proxy',
    'network_egress_enforcement',
    'sandbox_mount_isolation',
    'resource_limit_enforcement',
    'container_exit_and_child_cleanup',
    'host_database_access_denial',
  ]
}

function missingFrom(checks) {
  return checks
    .filter((item) => item.status === 'fail')
    .map(({ id, detail }) => ({ id, detail }))
}

function normalizeProfile(input) {
  if (!input || typeof input !== 'object' || Array.isArray(input)) {
    return { error: 'an explicit runtime profile is required' }
  }
  if (input.runtime !== undefined && input.runtime !== 'podman') {
    return { error: 'runtime must be podman' }
  }

  const image = input.image ?? input.image_digest
  if (typeof image !== 'string') {
    return { error: 'profile.image must be a pinned image reference' }
  }
  const match = PINNED_IMAGE.exec(image)
  if (!match?.groups) {
    return { error: 'profile.image must use an exact @sha256 digest' }
  }
  if (input.id !== undefined && (typeof input.id !== 'string' || input.id.length === 0)) {
    return { error: 'profile.id must be a non-empty string when supplied' }
  }
  if (input.cli_digest !== undefined && !/^sha256:[a-f0-9]{64}$/.test(input.cli_digest)) {
    return { error: 'profile.cli_digest must be a sha256 digest when supplied' }
  }

  return {
    profile: {
      ...(input.id ? { id: input.id } : {}),
      image,
      image_digest: match.groups.digest,
      ...(input.cli_digest ? { cli_digest: input.cli_digest } : {}),
    },
  }
}

function infoValue(info, ...paths) {
  for (const path of paths) {
    let value = info
    for (const key of path) value = value?.[key]
    if (value !== undefined) return value
  }
  return undefined
}

function imageDigestFromInspect(parsed) {
  const image = Array.isArray(parsed) ? parsed[0] : parsed
  return image?.Digest ?? image?.digest
}

function describeCommandFailure(result) {
  if (result?.timedOut) return 'command timed out'
  if (result?.errorCode === 'ENOENT') return 'podman executable was not found'
  if (result?.errorCode) return `podman could not be started (${result.errorCode})`
  return `command exited with ${result?.code ?? 'an unknown status'}`
}

/**
 * Default executor. The probe passes literal argv and never invokes a shell.
 */
export function executeBounded(file, args, { timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
  if (!validTimeout(timeoutMs)) return Promise.reject(new Error('timeoutMs must be an integer between 1 and 30000'))
  return new Promise((resolve) => {
    let settled = false
    const child = spawn(file, args, { shell: false, stdio: ['ignore', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    let timedOut = false
    let outputLimit = false
    let outputBytes = 0
    let killTimer
    const finish = (result) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      clearTimeout(killTimer)
      resolve(result)
    }
    const stop = () => {
      child.kill('SIGTERM')
      killTimer ??= setTimeout(() => child.kill('SIGKILL'), 100)
    }
    const timer = setTimeout(() => {
      timedOut = true
      stop()
    }, timeoutMs)
    child.stdout.setEncoding('utf8')
    child.stderr.setEncoding('utf8')
    const collect = (stream, chunk) => {
      outputBytes += Buffer.byteLength(chunk)
      if (outputBytes > 1024 * 1024) {
        outputLimit = true
        stop()
        return
      }
      if (stream === 'stdout') stdout += chunk
      else stderr += chunk
    }
    child.stdout.on('data', (chunk) => collect('stdout', chunk))
    child.stderr.on('data', (chunk) => collect('stderr', chunk))
    child.once('error', (error) => finish({ code: null, stdout, stderr, errorCode: error.code ?? 'spawn_error' }))
    // Resolve only after reaping the real subprocess, including timeout paths.
    child.once('close', (code) => finish({ code: outputLimit ? null : code, stdout, stderr, timedOut, ...(outputLimit ? { errorCode: 'output_limit' } : {}) }))
  })
}

async function callBounded(executor, file, args, timeoutMs) {
  // The real executor owns termination and must finish reaping before return.
  // Injected executors are process-free probes in deterministic tests.
  if (executor === executeBounded) return executor(file, args, { timeoutMs })
  let timer
  try {
    return await Promise.race([
      Promise.resolve(executor(file, args, { timeoutMs })),
      new Promise((resolve) => {
        timer = setTimeout(() => resolve({ code: null, stdout: '', stderr: '', timedOut: true }), timeoutMs)
      }),
    ])
  } catch (error) {
    return { code: null, stdout: '', stderr: '', errorCode: error?.code ?? 'executor_error' }
  } finally {
    clearTimeout(timer)
  }
}

/**
 * Run the prerequisite probe. `executor` is injectable so tests can be fully
 * deterministic and never need Podman or a network connection.
 */
export async function probeWorkflowRuntime({
  profile: rawProfile,
  platform = process.platform,
  executor = executeBounded,
  timeoutMs = DEFAULT_TIMEOUT_MS,
} = {}) {
  if (!validTimeout(timeoutMs)) {
    return capabilityUnavailable(null, [check('runtime_profile', 'fail', 'timeoutMs must be an integer between 1 and 30000')], [{ id: 'runtime_profile', detail: 'invalid probe timeout' }])
  }
  const normalized = normalizeProfile(rawProfile)
  if (normalized.error) {
    return capabilityUnavailable(null, [
      check('runtime_profile', 'fail', normalized.error),
      unavailable('host_linux', 'runtime profile is invalid'),
      unavailable('podman_callable', 'runtime profile is invalid'),
      unavailable('podman_rootless', 'runtime profile is invalid'),
      unavailable('cgroup_v2_resource_limits', 'runtime profile is invalid'),
      unavailable('pinned_image_present', 'runtime profile is invalid'),
    ], [{ id: 'runtime_profile', detail: normalized.error }])
  }

  const profile = normalized.profile
  const checks = [check('runtime_profile', 'pass', 'explicit Podman runtime profile supplied')]
  if (platform !== 'linux') {
    checks.push(
      check('host_linux', 'fail', `requires Linux host (received ${platform})`),
      unavailable('podman_callable', 'host is not Linux'),
      unavailable('podman_rootless', 'host is not Linux'),
      unavailable('cgroup_v2_resource_limits', 'host is not Linux'),
      unavailable('pinned_image_present', 'host is not Linux'),
    )
    return capabilityUnavailable(profile, checks, missingFrom(checks))
  }
  checks.push(check('host_linux', 'pass', 'Linux host detected'))

  const infoResult = await callBounded(executor, 'podman', ['info', '--format', 'json'], timeoutMs)
  if (infoResult?.code !== 0) {
    checks.push(
      check('podman_callable', 'fail', describeCommandFailure(infoResult)),
      unavailable('podman_rootless', 'podman is not callable'),
      unavailable('cgroup_v2_resource_limits', 'podman is not callable'),
      unavailable('pinned_image_present', 'podman is not callable'),
    )
    return capabilityUnavailable(profile, checks, missingFrom(checks))
  }
  checks.push(check('podman_callable', 'pass', 'podman info completed'))

  let info
  try {
    info = JSON.parse(infoResult.stdout)
  } catch {
    checks.push(
      check('podman_rootless', 'fail', 'podman info returned malformed JSON'),
      check('cgroup_v2_resource_limits', 'fail', 'podman info returned malformed JSON'),
    )
  }

  if (info) {
    const rootless = infoValue(info, ['host', 'security', 'rootless'], ['host', 'rootless'])
    checks.push(rootless === true
      ? check('podman_rootless', 'pass', 'Podman reports rootless execution')
      : check('podman_rootless', 'fail', 'Podman does not report rootless execution'))

    const cgroupVersion = infoValue(info, ['host', 'cgroupVersion'], ['host', 'cgroup_version'])
    const controllers = infoValue(info, ['host', 'cgroupControllers'], ['host', 'cgroup_controllers'])
    const missingControllers = Array.isArray(controllers)
      ? REQUIRED_CGROUP_CONTROLLERS.filter((controller) => !controllers.includes(controller))
      : REQUIRED_CGROUP_CONTROLLERS
    checks.push(cgroupVersion === 'v2' && missingControllers.length === 0
      ? check('cgroup_v2_resource_limits', 'pass', 'cgroup v2 exposes cpu, memory, and pids controllers')
      : check('cgroup_v2_resource_limits', 'fail', cgroupVersion !== 'v2'
        ? 'Podman does not report cgroup v2'
        : `required controllers unavailable: ${missingControllers.join(', ')}`))
  }

  const inspectResult = await callBounded(executor, 'podman', ['image', 'inspect', '--format', 'json', profile.image], timeoutMs)
  if (inspectResult?.code !== 0) {
    checks.push(check('pinned_image_present', 'fail', describeCommandFailure(inspectResult)))
  } else {
    try {
      const actualDigest = imageDigestFromInspect(JSON.parse(inspectResult.stdout))
      checks.push(actualDigest === profile.image_digest
        ? check('pinned_image_present', 'pass', 'specified image digest is present locally')
        : check('pinned_image_present', 'fail', 'local image digest does not match the supplied profile'))
    } catch {
      checks.push(check('pinned_image_present', 'fail', 'podman image inspect returned malformed JSON'))
    }
  }

  const missingCapabilities = missingFrom(checks)
  const report = {
    schema_version: 1,
    status: missingCapabilities.length === 0 ? 'prerequisite_ready' : 'capability_unavailable',
    runtime: 'podman',
    profile: receiptProfile(profile),
    checks,
    missing_capabilities: missingCapabilities,
    runtime_verified: false,
    remaining_evidence: remainingEvidence(),
  }
  return report
}

export async function readRuntimeProfile(argv, { read = readFile } = {}) {
  const args = [...argv]
  let profilePath
  let image
  let timeoutMs = DEFAULT_TIMEOUT_MS
  while (args.length > 0) {
    const option = args.shift()
    if (option === '--profile') profilePath = args.shift()
    else if (option === '--image-digest') image = args.shift()
    else if (option === '--timeout-ms') timeoutMs = Number(args.shift())
    else throw new Error('unknown runtime probe option')
  }
  if (!Number.isInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > 30_000) {
    throw new Error('timeout must be an integer between 1 and 30000 ms')
  }
  if (profilePath === undefined && image === undefined) {
    throw new Error('supply --profile <json-file> or --image-digest <image@sha256:...>')
  }
  let profile = {}
  if (profilePath !== undefined) {
    if (!profilePath) throw new Error('--profile requires a file path')
    try {
      profile = JSON.parse(await read(profilePath, 'utf8'))
    } catch {
      throw new Error('runtime profile could not be read as JSON')
    }
  }
  if (image !== undefined) {
    if (!image) throw new Error('--image-digest requires an image reference')
    profile = { ...profile, image }
  }
  return { profile, timeoutMs }
}

async function main() {
  try {
    const { profile, timeoutMs } = await readRuntimeProfile(process.argv.slice(2))
    const report = await probeWorkflowRuntime({ profile, timeoutMs })
    process.stdout.write(`${JSON.stringify(report)}\n`)
    process.exitCode = report.status === 'prerequisite_ready' ? 0 : 1
  } catch (error) {
    const report = capabilityUnavailable(null, [
      check('runtime_profile', 'fail', error.message),
    ], [{ id: 'runtime_profile', detail: error.message }])
    process.stdout.write(`${JSON.stringify(report)}\n`)
    process.exitCode = 1
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main()
}
