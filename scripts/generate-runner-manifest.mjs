import { createHash } from 'node:crypto'
import { createReadStream } from 'node:fs'
import { lstat, mkdir, open, rename, writeFile } from 'node:fs/promises'
import { basename, dirname, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

/** Pin the actual CI artifact before compiling the desktop app. Never fetch latest. */
export async function generateRunnerManifest({ archive, version, repository, output }) {
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version ?? '')) {
    throw new Error('A release version is required')
  }
  if (!/^[A-Za-z0-9][A-Za-z0-9-]*\/[A-Za-z0-9_][A-Za-z0-9_.-]*$/.test(repository ?? '')) {
    throw new Error('A GitHub owner/repository is required')
  }
  const asset = `praxis-runner_${version}_linux_x86_64.tar.gz`
  if (!archive || !output || basename(archive) !== asset) {
    throw new Error('Archive filename must match the pinned version')
  }
  const stat = await lstat(archive)
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size < 3 || stat.size > 512 * 1024 * 1024) {
    throw new Error('Runner archive must be a regular file of at most 512 MiB')
  }
  const file = await open(archive, 'r')
  try {
    const magic = Buffer.alloc(3)
    await file.read(magic, 0, 3, 0)
    if (!magic.equals(Buffer.from([0x1f, 0x8b, 8]))) throw new Error('Runner archive must be gzip')
  } finally {
    await file.close()
  }
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(archive)) hash.update(chunk)
  const manifest = {
    schema_version: 1,
    releases: [{
      platform: 'ubuntu-24.04',
      architecture: 'x86_64',
      version,
      url: `https://github.com/${repository}/releases/download/v${version}/${asset}`,
      sha256: hash.digest('hex'),
    }],
  }
  await mkdir(dirname(output), { recursive: true })
  const temporary = `${output}.${process.pid}.tmp`
  await writeFile(temporary, `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' })
  await rename(temporary, output)
  return manifest
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [archive, version, repository, output] = process.argv.slice(2)
  generateRunnerManifest({ archive, version, repository, output }).then(
    () => process.stdout.write('Pinned Runner manifest generated\n'),
    (error) => { process.stderr.write(`${error.message}\n`); process.exitCode = 1 },
  )
}
