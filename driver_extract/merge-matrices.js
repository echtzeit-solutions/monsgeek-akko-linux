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
// Output: data/device_matrices.json

const fs = require('fs');
const path = require('path');

const DATA_DIR = path.join(__dirname, '../data');
const DEVICES_FILE = path.join(DATA_DIR, 'devices.json');
const MATRICES_FILE = path.join(DATA_DIR, 'led_matrices.json');
const OUTPUT_FILE = path.join(DATA_DIR, 'device_matrices.json');

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
};

function main() {
  // Load data
  const devicesData = JSON.parse(fs.readFileSync(DEVICES_FILE, 'utf8'));
  const matricesData = JSON.parse(fs.readFileSync(MATRICES_FILE, 'utf8'));

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

      deviceMatrices[dev.id] = {
        name: dev.name,
        displayName: dev.displayName,
        keyLayoutName: dev.keyLayoutName || null,
        keyCount: matrix.filter(h => h !== 0).length,
        matchMethod,
        matrix: matrix,
        keyNames: keyNames,
      };
    } else {
      stats.byMethod.unmatched++;
    }
  }

  // Output
  const output = {
    version: 1,
    generatedAt: new Date().toISOString(),
    description: "Device ID to key matrix mapping. Matrix is position -> HID code, keyNames is position -> key name.",
    stats: {
      totalKeyboards: stats.total,
      matched: stats.matched,
      byMethod: stats.byMethod,
    },
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
