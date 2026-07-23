#!/usr/bin/env node
// Extract defaultMatrix from vendor driver device classes.
// Output: JSON with driver-class name -> HID code array, plus a device-name -> class alias map.
//
// Two vendor bundle formats are supported:
//
//   --refactored <dir>   Monolithic bundle (Akko Cloud v3/v4). One giant dist/index.*.js
//                        that webcrack + refactor-transform.js split into src/utils/*.js.
//                        Matrices appear as `rn(this, "defaultMatrix", [...])`; the
//                        name->class map comes from the switch in src/main.jsx.
//
//   --chunks <dir>       Vite code-split build (e.g. WOMIER 3.2.x). dist/js/ holds one
//                        chunk per device class; matrices are plain class fields
//                        (`defaultMatrix=[...]`) and the name->chunk->class map lives in
//                        per-chip-family loader chunks. No webcrack/refactor needed.
//
// Usage:
//   node extract-matrices.js --refactored <dir> [-o out.json]
//   node extract-matrices.js --chunks <dist/js dir> [-o out.json]

const fs = require('fs');
const path = require('path');

// The 4-byte-per-position matrix stores the HID usage at offset 2.
const HID_OFFSET_IN_ENTRY = 2;
const MATRIX_ENTRY_SIZE = 4;

function parseArgs(argv) {
  const opts = { refactored: null, chunks: null, output: null };
  for (let i = 0; i < argv.length; i++) {
    switch (argv[i]) {
      case '--refactored':
        opts.refactored = argv[++i];
        break;
      case '--chunks':
        opts.chunks = argv[++i];
        break;
      case '-o':
      case '--output':
        opts.output = argv[++i];
        break;
      case '-h':
      case '--help':
        console.log(fs.readFileSync(__filename, 'utf8').split('\n').slice(1, 20).join('\n'));
        process.exit(0);
        break;
      default:
        console.error(`Unknown option: ${argv[i]}`);
        process.exit(1);
    }
  }
  return opts;
}

// Pull the HID usage codes out of a `[b0,b1,hid,b3, ...]` matrix literal.
function hidCodesFromLiteral(literal) {
  const numbers = literal.split(',').map(n => parseInt(n.trim(), 10));
  const hidCodes = [];
  for (let i = 0; i + HID_OFFSET_IN_ENTRY < numbers.length; i += MATRIX_ENTRY_SIZE) {
    hidCodes.push(numbers[i + HID_OFFSET_IN_ENTRY]);
  }
  return hidCodes;
}

// `rn(this, "defaultMatrix", [...])` (refactored monolith) or `defaultMatrix=[...]`
// (class field, code-split build). The helper name varies per bundle, so match any.
function extractMatrix(content) {
  const match =
    content.match(/\w+\(this,\s*"defaultMatrix",\s*\[([\d\s,]+)\]\)/) ||
    content.match(/defaultMatrix\s*=\s*\[([\d\s,]+)\]/);
  return match ? hidCodesFromLiteral(match[1]) : null;
}

function extractClassInfo(content) {
  const match = content.match(/class\s+(\w+)\s+extends\s+(\w+)/);
  if (!match) return null;
  return { className: match[1], baseClass: match[2] };
}

// Determine chip family from a device/class name or base class
function getChipFamily(name, baseClass) {
  const lower = name.toLowerCase();
  if (lower.startsWith('ry5088') || baseClass?.includes('RY5088')) return 'RY5088';
  if (lower.startsWith('yc3123') || baseClass?.includes('YC3123')) return 'YC3123';
  if (lower.startsWith('yc3121') || baseClass?.includes('YC3121')) return 'YC3121';
  if (lower.startsWith('yc500') || baseClass?.includes('Yc500')) return 'YC500';
  if (lower.startsWith('yc300') || baseClass?.includes('Yc300')) return 'YC300';
  if (lower.startsWith('pan1086') || baseClass?.includes('Pan1086')) return 'Pan1086';
  if (lower.startsWith('ry6609') || baseClass?.includes('Ry6609')) return 'RY6609';
  if (lower.startsWith('ry1086') || baseClass?.includes('Ry1086')) return 'RY1086';
  if (lower.startsWith('ry3121') || baseClass?.includes('Ry3121')) return 'RY3121';
  if (lower.startsWith('ry5081') || baseClass?.includes('Ry5081')) return 'RY5081';
  if (lower.startsWith('ch585') || baseClass?.includes('Ch585')) return 'CH585';
  if (lower.startsWith('nrf54l') || baseClass?.includes('NRF54L')) return 'NRF54L';
  if (baseClass?.includes('Common')) return baseClass.replace('CommonKB', '').replace('_0001', '');
  return 'unknown';
}

// =============================================================================
// Monolithic bundle (webcrack + refactor-transform.js output)
// =============================================================================

// Parse the device-name -> driver-class switch from main.jsx.
// e.g. `case "ry5088_akko_tac75he_8k_dm": return new RY5088_mgk_fun75_8k_dm(g);`
// Many devices reuse another model's class (and thus its defaultMatrix); without
// this alias map a merge by name alone falls back to a generic layout.
function extractNameToClassFromMainJsx(mainJsx) {
  const map = {};
  let content;
  try {
    content = fs.readFileSync(mainJsx, 'utf8');
  } catch {
    console.warn(`main.jsx not found at ${mainJsx} — name->class aliases unavailable`);
    return map;
  }
  const re = /case\s+"([^"]+)":\s*return new (\w+)\(/g;
  for (let m; (m = re.exec(content)) !== null; ) map[m[1]] = m[2];
  return map;
}

function extractFromRefactored(refactoredDir) {
  const utilsDir = path.join(refactoredDir, 'src/utils');
  const mainJsx = path.join(refactoredDir, 'src/main.jsx');

  const matrices = {};
  const byFamily = {};

  for (const file of fs.readdirSync(utilsDir).filter(f => f.endsWith('.js'))) {
    const content = fs.readFileSync(path.join(utilsDir, file), 'utf8');
    const matrix = extractMatrix(content);
    if (!matrix || matrix.length === 0) continue;

    const classInfo = extractClassInfo(content);
    const className = classInfo?.className || file.replace(/\.js$/, '');
    const chipFamily = getChipFamily(file, classInfo?.baseClass);
    byFamily[chipFamily] = (byFamily[chipFamily] || 0) + 1;

    matrices[className] = {
      chipFamily,
      baseClass: classInfo?.baseClass || null,
      keyCount: matrix.filter(c => c !== 0).length,
      matrix,
    };
  }

  return { matrices, byFamily, nameToClass: extractNameToClassFromMainJsx(mainJsx) };
}

// =============================================================================
// Vite code-split build (one chunk per device class)
// =============================================================================

// Loader chunks map a device name to its lazily-imported class:
//   ry5088_womier_sk75he_europe_3m_8k_8k: () => t(() => import("./74d1dc6b.js"),
//     [...deps...], import.meta.url).then(_ => ({ default: _.Ry5088_womier_sk75he_europe_3m_8k_8k }))
// The dependency array between the import and the `.then` is unbounded in principle but
// only ever lists a handful of sibling chunks, so a bounded lazy span keeps this linear.
const LOADER_ENTRY_RE =
  /([A-Za-z0-9_]+):\(\)=>\w+\(\(\)=>import\("\.\/([0-9a-f]+\.js)"\)[\s\S]{0,800}?default:\w+\.([A-Za-z0-9_$]+)\}\)\)/g;

// `export{i as Ry5088_womier_sk75he_europe_3m_8k_8k}` — the minified local name (`i`)
// is meaningless, the export alias is the real class name.
function exportAliasFor(content, localName) {
  const re = new RegExp(`\\b${localName} as ([A-Za-z0-9_$]+)`);
  const m = content.match(re);
  return m ? m[1] : null;
}

function extractFromChunks(chunksDir) {
  const files = fs.readdirSync(chunksDir).filter(f => f.endsWith('.js'));
  const sources = new Map();
  for (const f of files) sources.set(f, fs.readFileSync(path.join(chunksDir, f), 'utf8'));

  // Pass 1: device name -> { chunk, class }
  const nameToClass = {};
  const nameToChunk = {};
  for (const [file, content] of sources) {
    if (!content.includes('import.meta.url')) continue;
    LOADER_ENTRY_RE.lastIndex = 0;
    let entries = 0;
    for (let m; (m = LOADER_ENTRY_RE.exec(content)) !== null; ) {
      const [, deviceName, chunk, className] = m;
      nameToClass[deviceName] = className;
      nameToChunk[deviceName] = chunk;
      entries++;
    }
    if (entries > 0) console.log(`  loader ${file}: ${entries} device entries`);
  }

  // Pass 2: chunk -> matrix, keyed by the class the chunk exports
  const chunkToClass = new Map();
  for (const [name, chunk] of Object.entries(nameToChunk)) chunkToClass.set(chunk, nameToClass[name]);

  const matrices = {};
  const byFamily = {};
  for (const [file, content] of sources) {
    const matrix = extractMatrix(content);
    if (!matrix || matrix.length === 0) continue;

    const classInfo = extractClassInfo(content);
    // Prefer the loader's class name; fall back to the chunk's own export alias so
    // classes that no device references still land in the output.
    const className =
      chunkToClass.get(file) ||
      (classInfo && exportAliasFor(content, classInfo.className)) ||
      classInfo?.className;
    if (!className) continue;

    const chipFamily = getChipFamily(className, classInfo?.baseClass);
    byFamily[chipFamily] = (byFamily[chipFamily] || 0) + 1;

    matrices[className] = {
      chipFamily,
      baseClass: null, // minified to a single letter in this format — not recoverable
      keyCount: matrix.filter(c => c !== 0).length,
      matrix,
    };
  }

  return { matrices, byFamily, nameToClass };
}

// =============================================================================

function main() {
  const opts = parseArgs(process.argv.slice(2));

  if (!opts.refactored === !opts.chunks) {
    console.error('Error: pass exactly one of --refactored <dir> or --chunks <dir>');
    process.exit(1);
  }

  if (!opts.output) {
    console.error('Error: -o <out.json> is required');
    process.exit(1);
  }
  const outputFile = opts.output;
  const { matrices, byFamily, nameToClass } = opts.chunks
    ? extractFromChunks(opts.chunks)
    : extractFromRefactored(opts.refactored);

  const output = {
    version: 1,
    generatedAt: new Date().toISOString(),
    format: opts.chunks ? 'chunks' : 'refactored',
    stats: {
      totalDevices: Object.keys(matrices).length,
      byChipFamily: byFamily,
      nameToClassEntries: Object.keys(nameToClass).length,
    },
    nameToClass,
    devices: matrices,
  };

  fs.mkdirSync(path.dirname(outputFile), { recursive: true });
  fs.writeFileSync(outputFile, JSON.stringify(output, null, 2));

  console.log(
    `Extracted ${output.stats.totalDevices} matrices ` +
      `(${output.stats.nameToClassEntries} name->class aliases) to ${outputFile}`
  );
  console.log('By chip family:');
  for (const [family, count] of Object.entries(byFamily).sort((a, b) => b[1] - a[1])) {
    console.log(`  ${family}: ${count}`);
  }
}

main();
