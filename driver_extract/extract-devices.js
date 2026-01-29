#!/usr/bin/env node
/**
 * MonsGeek Device Database Extractor
 *
 * Uses Babel to extract device definitions from minified JS bundles
 * (webapp or Electron driver) and outputs a unified JSON database.
 *
 * Usage:
 *   node extract-devices.js <input.js> [--output devices.json]
 *   node extract-devices.js --merge file1.json file2.json -o merged.json
 *
 * The extracted database includes:
 *   - id, vid, pid, name, displayName, type, company
 *   - keyLayout (parsed to key count)
 *   - layer, fnSysLayer
 *   - magnetism (hall effect support)
 *   - lightLayout, sideLightLayout (feature flags)
 *   - other.isSwitchReplaceable, other.noMagneticSwitch
 *   - other.travelSetting (hall effect calibration ranges)
 */

const fs = require('fs');
const path = require('path');
const parser = require('@babel/parser');
const traverse = require('@babel/traverse').default;
const generate = require('@babel/generator').default;
const t = require('@babel/types');

// =============================================================================
// CLI Argument Parsing
// =============================================================================

const args = process.argv.slice(2);
let inputFile = null;
let outputFile = null;
let mergeMode = false;
let mergeFiles = [];
let sourceTag = 'unknown';

for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--output' || arg === '-o') {
        outputFile = args[++i];
    } else if (arg === '--merge') {
        mergeMode = true;
    } else if (arg === '--source') {
        sourceTag = args[++i];
    } else if (arg === '--help' || arg === '-h') {
        console.log(`
MonsGeek Device Database Extractor

Usage:
  node extract-devices.js <input.js> [options]
  node extract-devices.js --merge <file1.json> <file2.json> ... [options]

Options:
  -o, --output <file>   Output JSON file (default: stdout)
  --source <tag>        Source tag for extracted devices (e.g., "webapp", "electron_v4")
  --merge               Merge multiple JSON device databases
  -h, --help            Show this help

Examples:
  # Extract from webapp bundle
  node extract-devices.js ../app.monsgeek.com/index.*.js -o webapp_devices.json --source webapp

  # Extract from Electron driver
  node extract-devices.js ./unbundled/deobfuscated.js -o electron_devices.json --source electron_v4

  # Merge databases
  node extract-devices.js --merge webapp.json electron.json -o devices.json
`);
        process.exit(0);
    } else if (!arg.startsWith('-')) {
        if (mergeMode) {
            mergeFiles.push(arg);
        } else {
            inputFile = arg;
        }
    }
}

// =============================================================================
// Merge Mode
// =============================================================================

if (mergeMode) {
    if (mergeFiles.length < 2) {
        console.error('Error: --merge requires at least 2 input files');
        process.exit(1);
    }

    console.error(`Merging ${mergeFiles.length} device databases...`);

    const deviceMap = new Map();
    const mergedKeyLayouts = {};

    for (const file of mergeFiles) {
        const data = JSON.parse(fs.readFileSync(file, 'utf-8'));
        const devices = data.devices || data;

        // Merge key layouts
        if (data.keyLayouts) {
            Object.assign(mergedKeyLayouts, data.keyLayouts);
            console.error(`  Loaded ${Object.keys(data.keyLayouts).length} key layouts from ${path.basename(file)}`);
        }

        for (const dev of devices) {
            const key = `${dev.vid}:${dev.pid}:${dev.id}`;
            const existing = deviceMap.get(key);

            if (existing) {
                // Merge sources
                const sources = new Set([
                    ...(existing.sources || []),
                    ...(dev.sources || []),
                ]);
                existing.sources = [...sources];

                // Prefer more detailed data (longer displayName, has features)
                if (!existing.keyCount && dev.keyCount) existing.keyCount = dev.keyCount;
                if (!existing.magnetism && dev.magnetism) existing.magnetism = dev.magnetism;
                if (!existing.sideLightLayout && dev.sideLightLayout) existing.sideLightLayout = dev.sideLightLayout;
                if (!existing.travelSetting && dev.travelSetting) existing.travelSetting = dev.travelSetting;
            } else {
                deviceMap.set(key, { ...dev });
            }
        }

        console.error(`  Loaded ${devices.length} devices from ${path.basename(file)}`);
    }

    const keyLayoutCount = Object.keys(mergedKeyLayouts).length;
    const merged = {
        version: 1,
        generatedAt: new Date().toISOString(),
        deviceCount: deviceMap.size,
        keyLayoutCount,
        devices: [...deviceMap.values()].sort((a, b) => a.id - b.id),
        keyLayouts: keyLayoutCount > 0 ? mergedKeyLayouts : undefined,
    };

    const output = JSON.stringify(merged, null, 2);
    if (outputFile) {
        fs.writeFileSync(outputFile, output);
        console.error(`Wrote ${merged.deviceCount} devices to ${outputFile}`);

        // Also write separate key_layouts.json
        if (keyLayoutCount > 0) {
            const layoutsFile = outputFile.replace(/\.json$/, '_layouts.json');
            const layoutsOutput = {
                version: 1,
                generatedAt: new Date().toISOString(),
                source: 'merged',
                count: keyLayoutCount,
                layouts: mergedKeyLayouts,
            };
            fs.writeFileSync(layoutsFile, JSON.stringify(layoutsOutput, null, 2));
            console.error(`Wrote ${keyLayoutCount} key layouts to ${layoutsFile}`);
        }
    } else {
        console.log(output);
    }
    process.exit(0);
}

// =============================================================================
// Extract Mode
// =============================================================================

if (!inputFile) {
    console.error('Error: No input file specified');
    console.error('Use --help for usage information');
    process.exit(1);
}

console.error(`\n=== MonsGeek Device Extractor ===`);
console.error(`Input: ${inputFile}`);
console.error(`Source: ${sourceTag}`);

// Read and parse
const code = fs.readFileSync(inputFile, 'utf-8');
console.error(`File size: ${(code.length / 1024 / 1024).toFixed(2)} MB`);

console.error('Parsing AST...');
const ast = parser.parse(code, {
    sourceType: 'module',
    plugins: ['jsx', 'typescript', 'classProperties', 'classPrivateProperties'],
    errorRecovery: true,
});

// =============================================================================
// Device Detection Helpers
// =============================================================================

function isDeviceObject(node) {
    if (!t.isObjectExpression(node)) return false;
    const props = new Set(
        node.properties
            .filter(p => t.isObjectProperty(p) && t.isIdentifier(p.key))
            .map(p => p.key.name)
    );
    // Must have core device properties
    return props.has('id') && props.has('vid') && props.has('pid') && props.has('name') && props.has('type');
}

function isDeviceArray(node) {
    if (!t.isArrayExpression(node)) return false;
    if (node.elements.length === 0) return false;
    // Check first few elements
    const checkCount = Math.min(3, node.elements.length);
    let deviceCount = 0;
    for (let i = 0; i < checkCount; i++) {
        if (isDeviceObject(node.elements[i])) deviceCount++;
    }
    return deviceCount >= Math.ceil(checkCount / 2);
}

// Extract primitive value from AST node
function extractValue(node) {
    if (!node) return undefined;

    if (t.isStringLiteral(node)) return node.value;
    if (t.isNumericLiteral(node)) return node.value;
    if (t.isBooleanLiteral(node)) return node.value;
    if (t.isNullLiteral(node)) return null;
    if (t.isIdentifier(node)) return `$ref:${node.name}`;

    // Handle unary expressions
    if (t.isUnaryExpression(node)) {
        // -N (negative numbers)
        if (node.operator === '-' && t.isNumericLiteral(node.argument)) {
            return -node.argument.value;
        }
        // !0 = true, !1 = false (minified booleans)
        if (node.operator === '!' && t.isNumericLiteral(node.argument)) {
            return node.argument.value === 0;
        }
    }

    if (t.isMemberExpression(node)) {
        const obj = t.isIdentifier(node.object) ? node.object.name : '?';
        const prop = t.isIdentifier(node.property) ? node.property.name : '?';
        return `$ref:${obj}.${prop}`;
    }

    if (t.isObjectExpression(node)) {
        const obj = {};
        for (const prop of node.properties) {
            if (t.isObjectProperty(prop)) {
                const key = t.isIdentifier(prop.key) ? prop.key.name :
                    t.isStringLiteral(prop.key) ? prop.key.value : null;
                if (key) {
                    obj[key] = extractValue(prop.value);
                }
            }
        }
        return obj;
    }

    if (t.isArrayExpression(node)) {
        return node.elements.map(el => extractValue(el));
    }

    return undefined;
}

// Extract device object from AST
function extractDevice(node) {
    if (!t.isObjectExpression(node)) return null;

    const device = {};

    for (const prop of node.properties) {
        if (!t.isObjectProperty(prop)) continue;

        const key = t.isIdentifier(prop.key) ? prop.key.name :
            t.isStringLiteral(prop.key) ? prop.key.value : null;
        if (!key) continue;

        const value = extractValue(prop.value);
        if (value !== undefined) {
            device[key] = value;
        }
    }

    return device;
}

// Parse key count from keyLayout reference
function parseKeyCount(keyLayoutRef) {
    if (typeof keyLayoutRef !== 'string') return null;
    // Format: "$ref:KeyLayout.Common68_MK636" or "$ref:KeyLayout.Common82"
    const match = keyLayoutRef.match(/Common(\d+)/);
    if (match) return parseInt(match[1], 10);

    // Special layouts
    if (keyLayoutRef.includes('Special29')) return 29;
    if (keyLayoutRef.includes('Special33')) return 33;

    return null;
}

// Normalize extracted device to clean JSON
function normalizeDevice(raw, source) {
    const device = {
        id: raw.id,
        vid: raw.vid,
        pid: raw.pid,
        name: raw.name,
        displayName: raw.displayName || raw.name,
        type: raw.type,
        company: raw.company || null,
        sources: [source],
    };

    // Key count from layout
    if (raw.keyLayout) {
        const keyCount = parseKeyCount(raw.keyLayout);
        if (keyCount) device.keyCount = keyCount;
        // Store raw layout name
        if (typeof raw.keyLayout === 'string') {
            device.keyLayoutName = raw.keyLayout.replace('$ref:KeyLayout.', '');
        }
    }

    // Layer info
    if (raw.layer) device.layer = raw.layer;
    if (raw.fnSysLayer) device.fnSysLayer = raw.fnSysLayer;

    // Feature flags
    if (raw.magnetism) device.magnetism = true;
    if (raw.lightLayout) device.hasLightLayout = true;
    if (raw.sideLightLayout) device.hasSideLight = true;

    // Other settings
    if (raw.other) {
        if (raw.other.isSwitchReplaceable) device.hotSwap = true;
        if (raw.other.noMagneticSwitch) device.noMagneticSwitch = true;
        if (raw.other.travelSetting) {
            device.travelSetting = raw.other.travelSetting;
        }
    }

    // Additional HID info
    if (raw.usage) device.usage = raw.usage;
    if (raw.usagePage) device.usagePage = raw.usagePage;

    return device;
}

// =============================================================================
// AST Traversal
// =============================================================================

const devices = [];
const deviceArrayNames = [];
const keyLayouts = {};
const keyCodes = {};  // keyCode array name -> array of key names
const layoutToKeyCode = {};  // KeyLayout name -> keyCode array name

console.error('Extracting device definitions...');

// Helper: Extract KeyLayout from IIFE patterns
// Pattern 1 (deobfuscated): (g => { g.X = "..."; return g; })({})
// Pattern 2 (minified): ((g) => ((g.X = "..."), (g.Y = "..."), g))(KeyLayout || {})
function extractKeyLayoutIIFE(node) {
    if (!t.isCallExpression(node)) return null;
    const callee = node.callee;
    if (!t.isArrowFunctionExpression(callee) && !t.isFunctionExpression(callee)) return null;

    // Extract assignments from function body
    const layouts = {};
    const body = callee.body;

    // Helper to extract from assignment expression
    function extractAssignment(expr) {
        if (!t.isAssignmentExpression(expr)) return;
        const left = expr.left;
        const right = expr.right;
        if (t.isMemberExpression(left) && t.isIdentifier(left.property)) {
            const layoutName = left.property.name;
            const value = extractValue(right);
            if (typeof value === 'string') {
                layouts[layoutName] = value;
            }
        }
    }

    // Pattern 2: Arrow with SequenceExpression body (minified)
    // ((g) => ((g.X = "..."), (g.Y = "..."), g))
    if (t.isSequenceExpression(body)) {
        for (const expr of body.expressions) {
            extractAssignment(expr);
        }
    }

    // Pattern 1: Arrow/function with BlockStatement body (deobfuscated)
    if (t.isBlockStatement(body)) {
        for (const stmt of body.body) {
            if (t.isExpressionStatement(stmt)) {
                if (t.isAssignmentExpression(stmt.expression)) {
                    extractAssignment(stmt.expression);
                }
                // Handle comma expressions: g.X = "a", g.Y = "b"
                if (t.isSequenceExpression(stmt.expression)) {
                    for (const expr of stmt.expression.expressions) {
                        extractAssignment(expr);
                    }
                }
            }
        }
    }

    return Object.keys(layouts).length > 0 ? layouts : null;
}

traverse(ast, {
    VariableDeclarator(nodePath) {
        const name = nodePath.node.id?.name;
        const init = nodePath.node.init;
        if (!name || !init) return;

        // Look for KeyLayout IIFE
        if (name === 'KeyLayout') {
            const layouts = extractKeyLayoutIIFE(init);
            if (layouts) {
                Object.assign(keyLayouts, layouts);
                console.error(`  Found KeyLayout: ${Object.keys(layouts).length} layouts`);
            }
        }

        // Look for keyCode arrays: const keyCode_* = [...] or const keyCodeAll = [...]
        if ((name.startsWith('keyCode') || name.startsWith('KeyCode')) && t.isArrayExpression(init)) {
            const keys = [];
            for (const el of init.elements) {
                if (t.isStringLiteral(el)) {
                    keys.push(el.value);
                }
            }
            if (keys.length > 0) {
                keyCodes[name] = keys;
            }
        }

        // Look for device arrays (support*Dev, SupportDev*, etc.)
        if (isDeviceArray(init)) {
            deviceArrayNames.push(name);
            console.error(`  Found device array: ${name} (${init.elements.length} items)`);

            for (const element of init.elements) {
                const raw = extractDevice(element);
                if (raw && raw.id && raw.vid !== undefined && raw.pid !== undefined) {
                    devices.push(normalizeDevice(raw, sourceTag));
                }
            }
        }
    },

    // Look for switch statements mapping KeyLayout to keyCode arrays
    // Pattern: switch (this.keyLayout) { case KeyLayout.X: return g(keyCode_Y); }
    SwitchStatement(nodePath) {
        const disc = nodePath.node.discriminant;
        // Check if this is a keyLayout switch
        if (!t.isMemberExpression(disc)) return;
        if (!t.isIdentifier(disc.property, { name: 'keyLayout' })) return;

        for (const caseNode of nodePath.node.cases) {
            if (!caseNode.test) continue; // skip default

            // Get KeyLayout name from case: KeyLayout.X
            let layoutName = null;
            if (t.isMemberExpression(caseNode.test) &&
                t.isIdentifier(caseNode.test.object, { name: 'KeyLayout' }) &&
                t.isIdentifier(caseNode.test.property)) {
                layoutName = caseNode.test.property.name;
            }
            if (!layoutName) continue;

            // Find return statement with keyCode reference
            for (const stmt of caseNode.consequent) {
                if (t.isReturnStatement(stmt) && t.isCallExpression(stmt.argument)) {
                    const args = stmt.argument.arguments;
                    if (args.length === 1 && t.isIdentifier(args[0])) {
                        const keyCodeName = args[0].name;
                        if (keyCodeName.startsWith('keyCode') || keyCodeName.startsWith('KeyCode')) {
                            layoutToKeyCode[layoutName] = keyCodeName;
                        }
                    }
                }
            }
        }

        if (Object.keys(layoutToKeyCode).length > 0) {
            console.error(`  Found KeyLayout→keyCode mapping: ${Object.keys(layoutToKeyCode).length} entries`);
        }
    },
});

console.error(`\nExtracted ${devices.length} devices from ${deviceArrayNames.length} arrays`);

// Deduplicate by id (keep first occurrence)
const uniqueMap = new Map();
for (const dev of devices) {
    const key = `${dev.vid}:${dev.pid}:${dev.id}`;
    if (!uniqueMap.has(key)) {
        uniqueMap.set(key, dev);
    }
}

const uniqueDevices = [...uniqueMap.values()].sort((a, b) => a.id - b.id);
console.error(`Unique devices: ${uniqueDevices.length}`);

// =============================================================================
// Output
// =============================================================================

const keyLayoutCount = Object.keys(keyLayouts).length;

const result = {
    version: 1,
    generatedAt: new Date().toISOString(),
    source: sourceTag,
    sourceFile: path.basename(inputFile),
    deviceArrays: deviceArrayNames,
    deviceCount: uniqueDevices.length,
    keyLayoutCount,
    devices: uniqueDevices,
    keyLayouts: keyLayoutCount > 0 ? keyLayouts : undefined,
};

const output = JSON.stringify(result, null, 2);

const keyCodeCount = Object.keys(keyCodes).length;
const layoutMappingCount = Object.keys(layoutToKeyCode).length;

if (outputFile) {
    fs.writeFileSync(outputFile, output);
    console.error(`\nWrote ${result.deviceCount} devices to ${outputFile}`);

    // Also write separate key_layouts.json if we found layouts
    if (keyLayoutCount > 0) {
        const layoutsFile = outputFile.replace(/\.json$/, '_layouts.json');
        const layoutsOutput = {
            version: 1,
            generatedAt: new Date().toISOString(),
            source: sourceTag,
            count: keyLayoutCount,
            layouts: keyLayouts,
        };
        fs.writeFileSync(layoutsFile, JSON.stringify(layoutsOutput, null, 2));
        console.error(`Wrote ${keyLayoutCount} key layouts to ${layoutsFile}`);
    }

    // Write keyCode arrays if found
    if (keyCodeCount > 0) {
        const keyCodesFile = outputFile.replace(/\.json$/, '_keycodes.json');
        const keyCodesOutput = {
            version: 1,
            generatedAt: new Date().toISOString(),
            source: sourceTag,
            arrayCount: keyCodeCount,
            mappingCount: layoutMappingCount,
            arrays: keyCodes,
            layoutMapping: layoutMappingCount > 0 ? layoutToKeyCode : undefined,
        };
        fs.writeFileSync(keyCodesFile, JSON.stringify(keyCodesOutput, null, 2));
        console.error(`Wrote ${keyCodeCount} keyCode arrays + ${layoutMappingCount} mappings to ${keyCodesFile}`);
    }
} else {
    console.log(output);
}

// Print summary stats
const stats = {
    keyboards: uniqueDevices.filter(d => d.type === 'keyboard').length,
    mice: uniqueDevices.filter(d => d.type === 'mouse').length,
    magnetism: uniqueDevices.filter(d => d.magnetism).length,
    sideLight: uniqueDevices.filter(d => d.hasSideLight).length,
    hotSwap: uniqueDevices.filter(d => d.hotSwap).length,
    keyLayouts: keyLayoutCount,
    keyCodes: keyCodeCount,
    layoutMappings: layoutMappingCount,
};

console.error(`\nStatistics:`);
console.error(`  Keyboards: ${stats.keyboards}`);
console.error(`  Mice: ${stats.mice}`);
console.error(`  Hall effect (magnetism): ${stats.magnetism}`);
console.error(`  Side lighting: ${stats.sideLight}`);
console.error(`  Hot-swap: ${stats.hotSwap}`);
console.error(`  Key layouts: ${stats.keyLayouts}`);
console.error(`  KeyCode arrays: ${stats.keyCodes}`);
console.error(`  Layout→KeyCode mappings: ${stats.layoutMappings}`);
