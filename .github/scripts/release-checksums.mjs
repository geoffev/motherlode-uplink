// SHA-256 of each installer, for the release: writes SHA256SUMS.txt and
// notes.md (the release notes with a checksum table added).
//
// Env: ARTIFACTS (tauri-action's artifactPaths, JSON), NOTES (release notes so far).
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { basename } from 'node:path'

const files = JSON.parse(process.env.ARTIFACTS).filter(f => /\.(exe|msi)$/i.test(f))
if (!files.length) throw new Error('No installers among the build artifacts')

// GitHub turns spaces in asset names into dots; list the names people download.
const rows = files.map(f => ({
  name: basename(f).replaceAll(' ', '.'),
  sha256: createHash('sha256').update(readFileSync(f)).digest('hex'),
}))

writeFileSync('SHA256SUMS.txt', rows.map(r => `${r.sha256}  ${r.name}`).join('\n') + '\n')

const notes = [
  process.env.NOTES,
  '',
  '### Checksums (SHA-256)',
  '',
  '| File | SHA-256 |',
  '|---|---|',
  ...rows.map(r => `| \`${r.name}\` | \`${r.sha256}\` |`),
  '',
  'To check a download in PowerShell, run `Get-FileHash .\\<file> -Algorithm SHA256` and compare it with the value above.',
  '',
]
writeFileSync('notes.md', notes.join('\n'))
console.log(rows.map(r => `${r.sha256}  ${r.name}`).join('\n'))
