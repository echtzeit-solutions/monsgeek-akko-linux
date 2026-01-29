#!/usr/bin/env node
// Extract defaultMatrix from all device class files
// Output: JSON with device name -> HID code array mapping

const fs = require('fs');
const path = require('path');

const UTILS_DIR = path.join(__dirname, 'refactored-v3/src/utils');
const OUTPUT_FILE = path.join(__dirname, '../data/led_matrices.json');

// Parse defaultMatrix from JS file content
function extractMatrix(content, filename) {
  // Match: rn(this, "defaultMatrix", [...])
  const match = content.match(/rn\(this,\s*"defaultMatrix",\s*\[([\d\s,]+)\]\)/);
  if (!match) return null;

  const numbers = match[1].split(',').map(n => parseInt(n.trim(), 10));

  // Extract HID codes (every 4th byte starting at index 2)
  // Format: [b0, b1, hid_code, b3, b0, b1, hid_code, b3, ...]
  const hidCodes = [];
  for (let i = 0; i < numbers.length; i += 4) {
    if (i + 2 < numbers.length) {
      hidCodes.push(numbers[i + 2]);
    }
  }

  return hidCodes;
}

// Extract class name and base class from JS file
function extractClassInfo(content) {
  const match = content.match(/class\s+(\w+)\s+extends\s+(\w+)/);
  if (!match) return null;
  return { className: match[1], baseClass: match[2] };
}

// Determine chip family from filename or base class
function getChipFamily(filename, baseClass) {
  const lower = filename.toLowerCase();
  if (lower.startsWith('ry5088') || baseClass?.includes('RY5088')) return 'RY5088';
  if (lower.startsWith('yc3123') || baseClass?.includes('YC3123')) return 'YC3123';
  if (lower.startsWith('yc3121') || baseClass?.includes('YC3121')) return 'YC3121';
  if (lower.startsWith('yc500') || baseClass?.includes('Yc500')) return 'YC500';
  if (lower.startsWith('yc300') || baseClass?.includes('Yc300')) return 'YC300';
  if (lower.startsWith('pan1086') || baseClass?.includes('Pan1086')) return 'Pan1086';
  if (lower.startsWith('ry6609') || baseClass?.includes('Ry6609')) return 'RY6609';
  if (lower.startsWith('ry1086') || baseClass?.includes('Ry1086')) return 'RY1086';
  if (lower.startsWith('ry3121') || baseClass?.includes('Ry3121')) return 'RY3121';
  if (baseClass?.includes('Common')) return baseClass.replace('CommonKB', '').replace('_0001', '');
  return 'unknown';
}

// Main extraction
function main() {
  const files = fs.readdirSync(UTILS_DIR).filter(f => f.endsWith('.js'));

  const matrices = {};
  const stats = { total: 0, extracted: 0, byFamily: {} };

  for (const file of files) {
    const filepath = path.join(UTILS_DIR, file);
    const content = fs.readFileSync(filepath, 'utf8');

    const matrix = extractMatrix(content, file);
    if (!matrix || matrix.length === 0) continue;

    const classInfo = extractClassInfo(content);
    const deviceName = classInfo?.className || file.replace('.js', '');
    const chipFamily = getChipFamily(file, classInfo?.baseClass);

    stats.total++;
    stats.extracted++;
    stats.byFamily[chipFamily] = (stats.byFamily[chipFamily] || 0) + 1;

    matrices[deviceName] = {
      chipFamily,
      baseClass: classInfo?.baseClass || null,
      keyCount: matrix.filter(c => c !== 0).length,
      matrix: matrix,
    };
  }

  // Create output
  const output = {
    version: 1,
    generatedAt: new Date().toISOString(),
    stats: {
      totalDevices: stats.extracted,
      byChipFamily: stats.byFamily,
    },
    devices: matrices,
  };

  // Ensure output directory exists
  const outDir = path.dirname(OUTPUT_FILE);
  if (!fs.existsSync(outDir)) {
    fs.mkdirSync(outDir, { recursive: true });
  }

  fs.writeFileSync(OUTPUT_FILE, JSON.stringify(output, null, 2));

  console.log(`Extracted ${stats.extracted} matrices to ${OUTPUT_FILE}`);
  console.log('By chip family:');
  for (const [family, count] of Object.entries(stats.byFamily).sort((a, b) => b[1] - a[1])) {
    console.log(`  ${family}: ${count}`);
  }
}

main();
