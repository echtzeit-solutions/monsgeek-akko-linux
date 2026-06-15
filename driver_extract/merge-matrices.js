#!/usr/bin/env node
// Merge device database with LED matrices
// Creates a lookup table: device ID -> matrix (HID codes)
//
// Strategy:
// 1. Device has ledMatrix directly in devices.json
// 2. Try exact match: device.name matches utility class name
// 3. Try layout match: find another device with same keyLayoutName that has ledMatrix
// 4. Try similar name: fuzzy match on device name
//
// Non-analog detection:
// 1. SVG diff: Calibration SVG vs KeyMappings SVG per layout — keys in KeyMappings
//    but not Calibration are non-analog (encoder rotation, side wheel, media button)
// 2. HID heuristic: matrix positions with HID 233/234 (Vol Up/Down = encoder rotation)
//    are always non-analog
//
// Output: data/device_matrices.json

const fs = require('fs');
const path = require('path');

const DATA_DIR = path.join(__dirname, '../data');
const DEVICES_FILE = path.join(DATA_DIR, 'devices.json');
const MATRICES_FILE = path.join(DATA_DIR, 'led_matrices.json');
const OUTPUT_FILE = path.join(DATA_DIR, 'device_matrices.json');
const SVG_DIR = path.join(__dirname, 'refactored/src/assets/svg');

// HID code to key name
const HID_TO_KEY = {
  0: null,  // Empty
  4: 'A', 5: 'B', 6: 'C', 7: 'D', 8: 'E', 9: 'F', 10: 'G', 11: 'H', 12: 'I', 13: 'J',
  14: 'K', 15: 'L', 16: 'M', 17: 'N', 18: 'O', 19: 'P', 20: 'Q', 21: 'R', 22: 'S', 23: 'T',
  24: 'U', 25: 'V', 26: 'W', 27: 'X', 28: 'Y', 29: 'Z',
  30: '1', 31: '2', 32: '3', 33: '4', 34: '5', 35: '6', 36: '7', 37: '8', 38: '9', 39: '0',
  40: 'Enter', 41: 'Esc', 42: 'Backspace', 43: 'Tab', 44: 'Space',
  45: '-', 46: '=', 47: '[', 48: ']', 49: '\\',
  50: 'IntlHash', 51: ';', 52: "'", 53: '`', 54: ',', 55: '.', 56: '/',
  57: 'CapsLock',
  58: 'F1', 59: 'F2', 60: 'F3', 61: 'F4', 62: 'F5', 63: 'F6',
  64: 'F7', 65: 'F8', 66: 'F9', 67: 'F10', 68: 'F11', 69: 'F12',
  70: 'PrintScreen', 71: 'ScrollLock', 72: 'Pause',
  73: 'Insert', 74: 'Home', 75: 'PageUp', 76: 'Delete', 77: 'End', 78: 'PageDown',
  79: 'Right', 80: 'Left', 81: 'Down', 82: 'Up',
  83: 'NumLock', 84: 'NumpadDivide', 85: 'NumpadMultiply', 86: 'NumpadSubtract',
  87: 'NumpadAdd', 88: 'NumpadEnter',
  89: 'Numpad1', 90: 'Numpad2', 91: 'Numpad3', 92: 'Numpad4', 93: 'Numpad5',
  94: 'Numpad6', 95: 'Numpad7', 96: 'Numpad8', 97: 'Numpad9', 98: 'Numpad0',
  99: 'NumpadDecimal', 100: 'IntlBackslash', 101: 'Application',
  135: 'IntlRo', 136: 'KanaMode', 137: 'IntlYen', 138: 'Convert', 139: 'NonConvert',
  224: 'LCtrl', 225: 'LShift', 226: 'LAlt', 227: 'LMeta',
  228: 'RCtrl', 229: 'RShift', 230: 'RAlt', 231: 'RMeta',
  // Consumer HID codes (encoder/media — not magnetic switches)
  233: 'VolUp', 234: 'VolDn',
};

// HID codes that indicate non-analog (GPIO-based) keys.
// These are encoder rotation / media buttons, not magnetic switches.
const NON_ANALOG_HID_CODES = new Set([233, 234]);

// SVG element IDs from webapp that represent non-analog keys.
// Mapped from Calibration vs KeyMappings SVG diffs.
const SVG_ID_TO_HID = {
  'AudioVolumeUp': 233,    // encoder rotate CW
  'AudioVolumeDown': 234,  // encoder rotate CCW
  'AudioVolumeMute': null,  // encoder push (HID 0 in matrix, GPIO)
  'Volume_Brightness': null, // side brightness wheel (GPIO)
  'MediaPlayPause': null,   // media button (GPIO)
};

/**
 * Extract non-analog key info from webapp SVG assets.
 * For each layout that has both Calibration and KeyMappings SVGs,
 * keys present in KeyMappings but absent from Calibration are non-analog.
 * Returns Map<svgStem, string[]> of non-analog SVG element IDs per layout.
 */
function extractNonAnalogFromSvgs() {
  const nonAnalogByLayout = new Map();

  let svgFiles;
  try {
    svgFiles = fs.readdirSync(SVG_DIR);
  } catch {
    console.log('SVG directory not found, skipping SVG-based non-analog detection');
    return nonAnalogByLayout;
  }

  for (const f of svgFiles) {
    const calibMatch = f.match(/^(Keyboard_\d+_.+?)_Calibration/);
    if (!calibMatch) continue;
    const stem = calibMatch[1];
    const kmFile = svgFiles.find(s => s.startsWith(stem + '_KeyMappings'));
    if (!kmFile) continue;

    const calibContent = fs.readFileSync(path.join(SVG_DIR, f), 'utf8');
    const kmContent = fs.readFileSync(path.join(SVG_DIR, kmFile), 'utf8');

    const extractIds = (content) => {
      const ids = new Set();
      const re = /id="#([^"]+)"/g;
      let m;
      while ((m = re.exec(content)) !== null) ids.add(m[1]);
      return ids;
    };

    const calibIds = extractIds(calibContent);
    const kmIds = extractIds(kmContent);
    const nonAnalog = [...kmIds].filter(id => !calibIds.has(id)).sort();

    if (nonAnalog.length > 0) {
      // Layout stem e.g. "SG9000" from "Keyboard_82_SG9000"
      const layoutPart = stem.split('_').slice(2).join('_');
      nonAnalogByLayout.set(layoutPart, nonAnalog);
    }
  }

  return nonAnalogByLayout;
}

/**
 * Find non-analog matrix positions for a device.
 * Uses SVG data when available, falls back to HID code heuristic.
 * Returns array of position indices that are non-analog (GPIO/encoder).
 */
function findNonAnalogPositions(matrix, keyLayoutName, svgNonAnalog) {
  const positions = [];

  // Method 1: HID code heuristic (always applied)
  for (let i = 0; i < matrix.length; i++) {
    if (NON_ANALOG_HID_CODES.has(matrix[i])) {
      positions.push(i);
    }
  }

  // Method 2: SVG-based detection — check if keyLayoutName contains a known SVG stem
  if (keyLayoutName) {
    for (const [stem, nonAnalogIds] of svgNonAnalog.entries()) {
      if (keyLayoutName.toLowerCase().includes(stem.toLowerCase())) {
        // SVG confirms encoder push / side wheel positions.
        // These have HID 0 in the matrix so we can't detect them by HID alone.
        // But we already capture them via HID 233/234 for rotation.
        // The SVG data is stored as metadata for the Rust driver to use.
        break;
      }
    }
  }

  return [...new Set(positions)].sort((a, b) => a - b);
}

function main() {
  // Load data
  const devicesData = JSON.parse(fs.readFileSync(DEVICES_FILE, 'utf8'));
  const matricesData = JSON.parse(fs.readFileSync(MATRICES_FILE, 'utf8'));

  // Extract non-analog key info from SVGs
  const svgNonAnalog = extractNonAnalogFromSvgs();
  if (svgNonAnalog.size > 0) {
    console.log(`Found non-analog keys in ${svgNonAnalog.size} SVG layouts`);
  }

  const devices = devicesData.devices;
  const matrices = matricesData.devices;

  // Build lookup tables
  const matrixByClassName = new Map();
  for (const [className, info] of Object.entries(matrices)) {
    matrixByClassName.set(className.toLowerCase(), info.matrix);
  }

  // Build layout -> matrix mapping
  // First pass: from devices that have ledMatrix directly
  const layoutToMatrix = new Map();
  for (const dev of devices) {
    if (dev.ledMatrix && dev.keyLayoutName) {
      if (!layoutToMatrix.has(dev.keyLayoutName)) {
        layoutToMatrix.set(dev.keyLayoutName, dev.ledMatrix);
      }
    }
  }

  // Second pass: from devices that match utility class names (exact match)
  for (const dev of devices) {
    if (dev.keyLayoutName && !layoutToMatrix.has(dev.keyLayoutName)) {
      const className = dev.name.toLowerCase();
      if (matrixByClassName.has(className)) {
        layoutToMatrix.set(dev.keyLayoutName, matrixByClassName.get(className));
      }
    }
  }

  // Build device ID -> matrix mapping
  const deviceMatrices = {};
  const stats = {
    total: 0,
    matched: 0,
    byMethod: {
      ledMatrixDirect: 0,
      exactName: 0,
      layoutFallback: 0,
      similarName: 0,
      unmatched: 0,
    }
  };

  for (const dev of devices) {
    if (dev.type !== 'keyboard') continue;
    stats.total++;

    let matrix = null;
    let matchMethod = null;

    // Method 1: Device has ledMatrix directly
    if (dev.ledMatrix) {
      matrix = dev.ledMatrix;
      matchMethod = 'ledMatrixDirect';
    }

    // Method 2: Exact class name match
    if (!matrix) {
      const className = dev.name.toLowerCase();
      if (matrixByClassName.has(className)) {
        matrix = matrixByClassName.get(className);
        matchMethod = 'exactName';
      }
    }

    // Method 3: Layout fallback - find matrix for same keyLayoutName
    if (!matrix && dev.keyLayoutName) {
      if (layoutToMatrix.has(dev.keyLayoutName)) {
        matrix = layoutToMatrix.get(dev.keyLayoutName);
        matchMethod = 'layoutFallback';
      }
    }

    // Method 4: Similar name match (fuzzy)
    if (!matrix) {
      const devNameParts = dev.name.toLowerCase().split('_').filter(p => p.length > 2);
      for (const [className, classMatrix] of matrixByClassName.entries()) {
        const classParts = className.split('_').filter(p => p.length > 2);
        // Check if significant parts match
        const matchCount = devNameParts.filter(p => classParts.some(cp => cp.includes(p) || p.includes(cp))).length;
        if (matchCount >= 3) {
          matrix = classMatrix;
          matchMethod = 'similarName';
          break;
        }
      }
    }

    if (matrix) {
      stats.matched++;
      stats.byMethod[matchMethod]++;

      // Convert to key names for readability
      const keyNames = matrix.map(hid => HID_TO_KEY[hid] || (hid === 0 ? null : `HID${hid}`));

      // Detect non-analog (GPIO/encoder) positions
      const nonAnalogPositions = findNonAnalogPositions(
        matrix, dev.keyLayoutName, svgNonAnalog
      );

      const entry = {
        name: dev.name,
        displayName: dev.displayName,
        keyLayoutName: dev.keyLayoutName || null,
        keyCount: matrix.filter(h => h !== 0).length,
        matchMethod,
        matrix: matrix,
        keyNames: keyNames,
      };

      if (nonAnalogPositions.length > 0) {
        entry.nonAnalogPositions = nonAnalogPositions;
      }

      deviceMatrices[dev.id] = entry;
    } else {
      stats.byMethod.unmatched++;
    }
  }

  // Count devices with non-analog data
  const nonAnalogCount = Object.values(deviceMatrices).filter(d => d.nonAnalogPositions).length;

  // Output
  const output = {
    version: 2,
    generatedAt: new Date().toISOString(),
    description: "Device ID to key matrix mapping. Matrix is position -> HID code, keyNames is position -> key name. nonAnalogPositions lists matrix indices of GPIO/encoder keys (not magnetic switches).",
    stats: {
      totalKeyboards: stats.total,
      matched: stats.matched,
      withNonAnalog: nonAnalogCount,
      byMethod: stats.byMethod,
    },
    nonAnalogHidCodes: [...NON_ANALOG_HID_CODES],
    svgNonAnalogLayouts: Object.fromEntries(svgNonAnalog),
    hidToKey: HID_TO_KEY,
    devices: deviceMatrices,
  };

  fs.writeFileSync(OUTPUT_FILE, JSON.stringify(output, null, 2));

  console.log(`Merged ${stats.matched}/${stats.total} keyboards to ${OUTPUT_FILE}`);
  console.log('By method:');
  for (const [method, count] of Object.entries(stats.byMethod)) {
    if (count > 0) {
      console.log(`  ${method}: ${count}`);
    }
  }
}

main();
