// Small-file benchmark: 5,000 files of 1 KiB in 50 directories.
// Phases: create, read, stat, delete. Sequential sync fs calls, like most CLI tools.
// Usage: node smallfiles.mjs <targetDir> <label>
// Prints: RESULT;<label>;smallfiles_<phase>;<seconds>;
import fs from 'node:fs'
import path from 'node:path'

const [target, label = 'unknown'] = process.argv.slice(2)
if (!target) {
  console.error('usage: node smallfiles.mjs <targetDir> <label>')
  process.exit(2)
}

const root = path.join(target, 'smallfiles')
const DIRS = 50
const FILES_PER_DIR = 100
const payload = Buffer.alloc(1024, 'x')

function timed(name, fn) {
  const t0 = process.hrtime.bigint()
  fn()
  const s = Number(process.hrtime.bigint() - t0) / 1e9
  console.log(`RESULT;${label};smallfiles_${name};${s.toFixed(3)};`)
}

fs.rmSync(root, { recursive: true, force: true })

timed('create', () => {
  for (let d = 0; d < DIRS; d++) {
    const dir = path.join(root, `d${d}`)
    fs.mkdirSync(dir, { recursive: true })
    for (let f = 0; f < FILES_PER_DIR; f++) fs.writeFileSync(path.join(dir, `f${f}.txt`), payload)
  }
})

timed('read', () => {
  let bytes = 0
  for (const d of fs.readdirSync(root)) {
    for (const f of fs.readdirSync(path.join(root, d))) bytes += fs.readFileSync(path.join(root, d, f)).length
  }
  if (bytes !== DIRS * FILES_PER_DIR * 1024) throw new Error(`read ${bytes} bytes`)
})

timed('stat', () => {
  for (const d of fs.readdirSync(root)) {
    for (const f of fs.readdirSync(path.join(root, d))) fs.statSync(path.join(root, d, f))
  }
})

timed('delete', () => fs.rmSync(root, { recursive: true, force: true }))
