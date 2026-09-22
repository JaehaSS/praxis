/**
 * Fail-closed entry point for T00. This is intentionally a prerequisite
 * receipt, not a container or vendor smoke: no profile means no pass.
 * For supervised live checks use praxis-runner with PRAXIS_WORKFLOW_CHECK_ONLY=1
 * and PRAXIS_WORKFLOW_CONFIG; see deploy/praxis-workflow.example.json.
 */
import { pathToFileURL } from 'node:url'
import { probeWorkflowRuntime, readRuntimeProfile } from '../workflow-runtime-probe.mjs'

export async function runWorkflowRuntimeSmoke(argv = process.argv.slice(2), dependencies = {}) {
  try {
    const { profile, timeoutMs } = await readRuntimeProfile(argv, dependencies)
    return await probeWorkflowRuntime({
      profile,
      timeoutMs,
      ...(dependencies.platform ? { platform: dependencies.platform } : {}),
      ...(dependencies.executor ? { executor: dependencies.executor } : {}),
    })
  } catch (error) {
    return {
      schema_version: 1,
      status: 'capability_unavailable',
      runtime: 'podman',
      profile: null,
      checks: [{ id: 'runtime_profile', status: 'fail', detail: error.message }],
      missing_capabilities: [{ id: 'runtime_profile', detail: error.message }],
      runtime_verified: false,
      remaining_evidence: [
        'vendor_auth_and_attempt_proxy',
        'network_egress_enforcement',
        'sandbox_mount_isolation',
        'resource_limit_enforcement',
        'container_exit_and_child_cleanup',
        'host_database_access_denial',
      ],
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const report = await runWorkflowRuntimeSmoke()
  process.stdout.write(`${JSON.stringify(report)}\n`)
  process.exitCode = report.status === 'prerequisite_ready' ? 0 : 1
}
