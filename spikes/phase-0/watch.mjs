// Logs file-change events for a directory: inotify-based fs.watch and stat polling side by side.
// Usage: node watch.mjs <dir>
import fs from 'node:fs'
import path from 'node:path'

const dir = process.argv[2] ?? '/w'
const t0 = Date.now()
const log = (src, msg) => console.log(`${((Date.now() - t0) / 1000).toFixed(2)}s ${src} ${msg}`)

fs.watch(dir, (event, file) => log('fs.watch', `${event} ${file}`))

// Polling fallback (what CHOKIDAR_USEPOLLING / WATCHPACK_POLLING boil down to).
const seen = new Map()
setInterval(() => {
  for (const f of fs.readdirSync(dir)) {
    const st = fs.statSync(path.join(dir, f))
    const sig = `${st.size}:${st.mtimeMs}`
    if (seen.get(f) !== sig) {
      if (seen.has(f) || t0 + 1500 < Date.now()) log('poll', `changed ${f}`)
      seen.set(f, sig)
    }
  }
}, 500)

log('ready', dir)
