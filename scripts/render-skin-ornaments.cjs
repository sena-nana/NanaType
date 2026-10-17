// Development tool only: requires sharp; SVG is never interpreted by the app.
const fs = require('node:fs/promises');
const path = require('node:path');
const sharp = require('sharp');
(async () => {
  const names = process.argv.slice(2);
  for (const name of names.length ? names : ['gothic', 'celestial', 'forest', 'tidal', 'lantern', 'citrus']) {
    const dir = path.join(__dirname, '..', 'crates', 'theme', 'builtin', name);
    const svg = await fs.readFile(path.join(dir, 'ornament.svg'));
    const meta = await sharp(svg).metadata();
    await sharp(svg).resize(meta.width * 2, meta.height * 2).png().toFile(path.join(dir, 'ornament.png'));
    process.stdout.write(`${name}: ${meta.width * 2} x ${meta.height * 2}\n`);
  }
})().catch(e => { process.stderr.write(String(e)); process.exitCode = 1; });
