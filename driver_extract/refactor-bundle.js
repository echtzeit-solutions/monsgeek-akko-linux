#!/usr/bin/env node
/**
 * MonsGeek/Epomaker IOT Driver Bundle Refactorer
 *
 * Uses Babel AST for robust extraction of:
 * - Device definitions (with all their properties)
 * - SVG strings (template literals and string literals)
 * - CSS strings (styled-components, emotion, etc.)
 * - React components
 * - HID protocol classes
 * - Vendored library detection
 *
 * Usage: node refactor-bundle.js [input.js] [output-dir]
 */

const fs = require('fs');
const path = require('path');
const parser = require('@babel/parser');
const traverse = require('@babel/traverse').default;
const generate = require('@babel/generator').default;
const t = require('@babel/types');

// Configuration
const DEFAULT_INPUT = './unbundled/deobfuscated.js';
const DEFAULT_OUTPUT = './refactored';

const inputFile = process.argv[2] || DEFAULT_INPUT;
const outputDir = process.argv[3] || DEFAULT_OUTPUT;

console.log('=== MonsGeek Driver Bundle Refactorer ===');
console.log(`Input: ${inputFile}`);
console.log(`Output: ${outputDir}`);

// Known vendored libraries to detect
const KNOWN_LIBRARIES = {
    'react': { patterns: [/React\.createElement/, /__SECRET_INTERNALS_DO_NOT_USE/], version: '^18.2.0' },
    'react-dom': { patterns: [/ReactDOM\.render/, /ReactDOM\.createRoot/], version: '^18.2.0' },
    'lodash': { patterns: [/\b_\.map\b|\b_\.filter\b/, /lodash/i], version: '^4.17.21' },
    'ramda': { patterns: [/\bR\.map\b|\bR\.compose\b/], version: '^0.29.0' },
    'axios': { patterns: [/axios\.(get|post)/], version: '^1.6.0' },
    'pngjs': { patterns: [/PNG\.sync/, /\bnew PNG\b/], version: '^7.0.0' },
    'pako': { patterns: [/pako\.(inflate|deflate)/], version: '^2.1.0' },
    'protobufjs': { patterns: [/protobuf\./, /Message\.create/], version: '^7.2.0' },
    'electron': { patterns: [/ipcRenderer/, /ipcMain/], version: '^28.0.0', dev: true },
    '@emotion/react': { patterns: [/emotion/, /css`/, /styled\./], version: '^11.11.0' },
    'rxjs': { patterns: [/\bSubject\b/, /\.subscribe\(/], version: '^7.8.0' },
    'i18next': { patterns: [/i18n\./, /useTranslation/], version: '^23.7.0' },
    'tinycolor2': { patterns: [/tinycolor/i], version: '^1.6.0' },
    'omggif': { patterns: [/GifWriter|GifReader/], version: '^1.0.10' },
    'image-q': { patterns: [/applyPalette|buildPalette/], version: '^4.0.0' },
};

// Create output directories
const dirs = {
    root: outputDir,
    src: path.join(outputDir, 'src'),
    components: path.join(outputDir, 'src/components'),
    svg: path.join(outputDir, 'src/assets/svg'),
    css: path.join(outputDir, 'src/assets/css'),
    devices: path.join(outputDir, 'src/devices'),
    protocol: path.join(outputDir, 'src/protocol'),
};

Object.values(dirs).forEach(dir => {
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
});

// Read input file
console.log('\nReading input file...');
const code = fs.readFileSync(inputFile, 'utf-8');
console.log(`File size: ${(code.length / 1024 / 1024).toFixed(2)} MB`);

// Track extracted items
const extracted = {
    svgs: [],
    css: [],
    components: [],
    classes: [],
    devices: [],
};

// Step 1: Detect vendored libraries
console.log('\n=== Step 1: Detecting vendored libraries ===');
const dependencies = {};
const devDependencies = {};

Object.entries(KNOWN_LIBRARIES).forEach(([lib, config]) => {
    if (config.patterns.some(p => p.test(code))) {
        (config.dev ? devDependencies : dependencies)[lib] = config.version;
        console.log(`  ✓ ${lib} (${config.version})`);
    }
});

// Step 2: Parse AST
console.log('\n=== Step 2: Parsing AST ===');
let ast;
try {
    ast = parser.parse(code, {
        sourceType: 'module',
        plugins: ['jsx', 'typescript', 'classProperties', 'classPrivateProperties'],
        errorRecovery: true,
    });
    console.log('AST parsed successfully');
} catch (e) {
    console.error('AST parsing failed:', e.message);
    process.exit(1);
}

// Collections for AST extraction
const svgStrings = [];
const cssStrings = [];
const deviceDefinitions = [];
const deviceArrays = []; // Named arrays like supportRY5088Dev
const componentDefinitions = [];
const classDefinitions = [];

// Step 3: AST Traversal - Extract everything in one pass
console.log('\n=== Step 3: AST Traversal ===');

traverse(ast, {
    // Extract SVG strings from string literals and template literals
    StringLiteral(nodePath) {
        const value = nodePath.node.value;
        if (value.includes('<svg') && value.length > 500) {
            svgStrings.push({ value, type: 'string' });
        }
    },

    TemplateLiteral(nodePath) {
        // Get the full quasi value
        const quasis = nodePath.node.quasis.map(q => q.value.raw).join('');
        if (quasis.includes('<svg') && quasis.length > 500) {
            svgStrings.push({ value: quasis, type: 'template' });
        }
        // CSS in template literals (styled-components pattern)
        if (quasis.includes('{') && quasis.includes(':') && quasis.includes(';') && quasis.length > 200) {
            // Check if it looks like CSS
            if (/\w+\s*:\s*[^;]+;/.test(quasis)) {
                cssStrings.push({ value: quasis, type: 'template' });
            }
        }
    },

    // Tagged template expressions (styled.div`...`, css`...`)
    TaggedTemplateExpression(nodePath) {
        const tag = nodePath.node.tag;
        const quasi = nodePath.node.quasi;

        // Check for styled-components or emotion patterns
        let isStyled = false;
        if (t.isMemberExpression(tag) && tag.object?.name === 'styled') {
            isStyled = true;
        } else if (t.isIdentifier(tag) && (tag.name === 'css' || tag.name === 'keyframes')) {
            isStyled = true;
        } else if (t.isCallExpression(tag) && t.isMemberExpression(tag.callee)) {
            isStyled = tag.callee.object?.name === 'styled';
        }

        if (isStyled) {
            const cssValue = quasi.quasis.map(q => q.value.raw).join('${...}');
            if (cssValue.length > 100) {
                cssStrings.push({ value: cssValue, type: 'styled' });
            }
        }
    },

    // Extract device definitions from object expressions
    ObjectExpression(nodePath) {
        const props = nodePath.node.properties;
        if (props.length < 3) return;

        // Look for device definition pattern: has id, vid, pid, name, type
        const propNames = new Set(props.filter(p => t.isObjectProperty(p) && t.isIdentifier(p.key))
            .map(p => p.key.name));

        if (propNames.has('id') && propNames.has('vid') && propNames.has('pid') &&
            propNames.has('name') && propNames.has('type')) {

            try {
                const device = {};
                props.forEach(prop => {
                    if (!t.isObjectProperty(prop) || !t.isIdentifier(prop.key)) return;
                    const key = prop.key.name;
                    const val = prop.value;

                    if (key === 'id' && t.isNumericLiteral(val)) {
                        device.id = val.value;
                    } else if (key === 'vid' && t.isNumericLiteral(val)) {
                        device.vid = val.value;
                        device.vidHex = '0x' + val.value.toString(16).padStart(4, '0');
                    } else if (key === 'pid' && t.isNumericLiteral(val)) {
                        device.pid = val.value;
                        device.pidHex = '0x' + val.value.toString(16).padStart(4, '0');
                    } else if (key === 'name' && t.isStringLiteral(val)) {
                        device.name = val.value;
                    } else if (key === 'displayName' && t.isStringLiteral(val)) {
                        device.displayName = val.value;
                    } else if (key === 'type' && t.isStringLiteral(val)) {
                        device.type = val.value;
                    } else if (key === 'company' && t.isStringLiteral(val)) {
                        device.company = val.value;
                    } else if (key === 'layer' && t.isNumericLiteral(val)) {
                        device.layer = val.value;
                    } else if (key === 'magnetism' && t.isBooleanLiteral(val)) {
                        device.magnetism = val.value;
                    } else if (key === 'sensor' && t.isStringLiteral(val)) {
                        device.sensor = val.value;
                    }
                });

                if (device.id !== undefined && device.vid && device.pid && device.name && device.type) {
                    deviceDefinitions.push(device);
                }
            } catch (e) {
                // Skip malformed definitions
            }
        }
    },

    // Extract class declarations
    ClassDeclaration(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name) return;

        const nodeCode = generate(nodePath.node).code;
        const superClass = nodePath.node.superClass?.name ||
                          nodePath.node.superClass?.property?.name;

        const isReact = superClass?.includes('Component') ||
                       nodeCode.includes('render()') ||
                       nodeCode.includes('componentDidMount');

        const isHID = name.toLowerCase().includes('hid') ||
                     name.toLowerCase().includes('interface') ||
                     nodeCode.includes('sendReport') ||
                     nodeCode.includes('navigator.hid') ||
                     nodeCode.includes('whoAmI');

        classDefinitions.push({ name, isReact, isHID, superClass, code: nodeCode, size: nodeCode.length });
    },

    // Extract function components
    FunctionDeclaration(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name || name.length < 3) return;

        const nodeCode = generate(nodePath.node).code;
        const isPascal = /^[A-Z]/.test(name);
        const hasJSX = nodeCode.includes('createElement') || nodeCode.includes('jsx(');
        const hasHooks = nodeCode.includes('useState') || nodeCode.includes('useEffect');

        if (isPascal && (hasJSX || hasHooks)) {
            componentDefinitions.push({ name, code: nodeCode, size: nodeCode.length, type: 'function' });
        }
    },

    // Extract arrow function components
    VariableDeclarator(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name) return;

        const init = nodePath.node.init;
        if (!init) return;

        // Check for device arrays (e.g., supportRY5088Dev, SupportDev, etc.)
        if (t.isArrayExpression(init) && init.elements.length > 0) {
            // Check if name looks like a device array
            const isDeviceArrayName = /[Ss]upport.*[Dd]ev|[Dd]ev(ice)?s?$/i.test(name);

            if (isDeviceArrayName) {
                // Check if first element looks like a device object
                const firstEl = init.elements[0];
                if (t.isObjectExpression(firstEl)) {
                    const propNames = new Set(firstEl.properties
                        .filter(p => t.isObjectProperty(p) && t.isIdentifier(p.key))
                        .map(p => p.key.name));

                    if (propNames.has('id') && propNames.has('vid') && propNames.has('pid')) {
                        const nodeCode = generate(nodePath.node).code;
                        deviceArrays.push({
                            name,
                            code: `const ${nodeCode}`,
                            count: init.elements.length,
                            size: nodeCode.length
                        });
                    }
                }
            }
        }

        // Arrow function components
        if (!t.isArrowFunctionExpression(init) && !t.isFunctionExpression(init)) return;

        const nodeCode = generate(nodePath.node).code;
        const isPascal = /^[A-Z]/.test(name);
        const hasJSX = nodeCode.includes('createElement') || nodeCode.includes('jsx(');
        const hasHooks = nodeCode.includes('useState') || nodeCode.includes('useEffect');

        if (isPascal && (hasJSX || hasHooks) && nodeCode.length > 100) {
            componentDefinitions.push({ name, code: `const ${nodeCode}`, size: nodeCode.length, type: 'arrow' });
        }
    },
});

console.log(`  SVG strings: ${svgStrings.length}`);
console.log(`  CSS strings: ${cssStrings.length}`);
console.log(`  Device definitions: ${deviceDefinitions.length}`);
console.log(`  Device arrays: ${deviceArrays.length} (${deviceArrays.reduce((a, b) => a + b.count, 0)} devices total)`);
console.log(`  Class definitions: ${classDefinitions.length}`);
console.log(`  Component definitions: ${componentDefinitions.length}`);

// Step 4: Write extracted SVGs
console.log('\n=== Step 4: Writing SVG files ===');
const svgExports = [];
svgStrings.forEach((svg, idx) => {
    // Try to extract a name from the SVG
    const idMatch = svg.value.match(/id="([^"]+)"/);
    let svgName = idMatch?.[1] || `svg_${idx}`;
    svgName = svgName.replace(/[^a-zA-Z0-9_]/g, '_').substring(0, 50);
    // Prefix reserved words
    const reserved = ['default', 'class', 'function', 'return', 'if', 'else', 'for', 'while', 'switch', 'case', 'break', 'continue', 'var', 'let', 'const', 'import', 'export', 'new', 'this', 'delete', 'typeof', 'void', 'null', 'undefined', 'true', 'false'];
    if (reserved.includes(svgName) || /^\d/.test(svgName)) svgName = `svg_${svgName}`;

    // Avoid duplicates
    if (svgExports.some(s => s.name === svgName)) {
        svgName = `${svgName}_${idx}`;
    }

    fs.writeFileSync(path.join(dirs.svg, `${svgName}.svg`), svg.value);
    // Escape for template literal: backslashes first, then backticks, then ${
    const escaped = svg.value
        .replace(/\\/g, '\\\\')
        .replace(/`/g, '\\`')
        .replace(/\$\{/g, '\\${');
    fs.writeFileSync(path.join(dirs.svg, `${svgName}.js`),
        `export const ${svgName} = \`${escaped}\`;\nexport default ${svgName};\n`);

    svgExports.push({ name: svgName });
    extracted.svgs.push({ name: svgName, size: svg.value.length });
});
console.log(`Wrote ${extracted.svgs.length} SVG files`);

// SVG index
fs.writeFileSync(path.join(dirs.svg, 'index.js'),
    svgExports.map(s => `export { ${s.name} } from './${s.name}';`).join('\n') || '// No SVGs');

// Step 5: Write extracted CSS
console.log('\n=== Step 5: Writing CSS files ===');
cssStrings.forEach((css, idx) => {
    const filename = `styles_${idx}`;
    fs.writeFileSync(path.join(dirs.css, `${filename}.css`), css.value);
    // Escape for template literal: backslashes first, then backticks, then ${
    const escapedCss = css.value
        .replace(/\\/g, '\\\\')
        .replace(/`/g, '\\`')
        .replace(/\$\{/g, '\\${');
    fs.writeFileSync(path.join(dirs.css, `${filename}.js`),
        `export const ${filename} = \`${escapedCss}\`;\nexport default ${filename};\n`);
    extracted.css.push({ name: filename, size: css.value.length });
});
console.log(`Wrote ${extracted.css.length} CSS files`);

// Step 6: Write device definitions
console.log('\n=== Step 6: Writing device definitions ===');

// Deduplicate by id
const uniqueDevices = new Map();
deviceDefinitions.forEach(d => {
    if (!uniqueDevices.has(d.id)) {
        uniqueDevices.set(d.id, d);
    }
});
const devicesArray = [...uniqueDevices.values()];

fs.writeFileSync(path.join(dirs.devices, 'devices.json'), JSON.stringify(devicesArray, null, 2));
fs.writeFileSync(path.join(dirs.devices, 'index.js'), `
// Auto-extracted device definitions (${devicesArray.length} devices)
export const devices = ${JSON.stringify(devicesArray, null, 2)};

export const keyboards = devices.filter(d => d.type === 'keyboard');
export const mice = devices.filter(d => d.type === 'mouse');
export const audio = devices.filter(d => d.type === 'audio');

export const getDeviceById = (id) => devices.find(d => d.id === id);
export const getDeviceByVidPid = (vid, pid) => devices.find(d => d.vid === vid && d.pid === pid);
export const getDeviceByName = (name) => devices.find(d => d.name === name || d.displayName === name);

export default devices;
`);

console.log(`Wrote ${devicesArray.length} unique device definitions`);
extracted.devices = devicesArray;

// Step 6b: Write device arrays (with full code including references)
console.log('\n=== Step 6b: Writing device arrays ===');
deviceArrays.forEach(arr => {
    fs.writeFileSync(path.join(dirs.devices, `${arr.name}.js`),
        `// Auto-extracted device array: ${arr.name}\n// Contains ${arr.count} device definitions\n// Note: References like KeyLayout.*, lightXX may need resolution\n\n${arr.code};\n\nexport default ${arr.name};\n`);
});
console.log(`Wrote ${deviceArrays.length} device arrays:`);
deviceArrays.forEach(arr => console.log(`  • ${arr.name}: ${arr.count} devices`));

// Step 7: Write React components
console.log('\n=== Step 7: Writing React components ===');
const reactComponents = [
    ...classDefinitions.filter(c => c.isReact),
    ...componentDefinitions,
];

reactComponents.forEach(comp => {
    const filename = `${comp.name}.jsx`;
    let imports = "import React from 'react';\n";
    if (comp.code.includes('useState')) imports += "import { useState } from 'react';\n";
    if (comp.code.includes('useEffect')) imports += "import { useEffect } from 'react';\n";

    fs.writeFileSync(path.join(dirs.components, filename),
        `// Auto-extracted: ${comp.name}\n${imports}\n${comp.code}\n\nexport default ${comp.name};\n`);
    extracted.components.push({ name: comp.name, size: comp.size });
});
console.log(`Wrote ${extracted.components.length} React components`);

// Components index
fs.writeFileSync(path.join(dirs.components, 'index.js'),
    extracted.components.map(c => `export { default as ${c.name} } from './${c.name}';`).join('\n') || '// No components');

// Step 8: Write HID/Protocol classes
console.log('\n=== Step 8: Writing HID/Protocol classes ===');
const hidClasses = classDefinitions.filter(c => c.isHID);
hidClasses.forEach(cls => {
    fs.writeFileSync(path.join(dirs.protocol, `${cls.name}.js`),
        `// Auto-extracted HID class: ${cls.name}\n// Extends: ${cls.superClass || 'none'}\n\n${cls.code}\n\nexport default ${cls.name};\n`);
    extracted.classes.push({ name: cls.name, type: 'hid' });
});
console.log(`Wrote ${extracted.classes.length} HID classes`);

// Protocol index
fs.writeFileSync(path.join(dirs.protocol, 'index.js'),
    extracted.classes.map(c => `export { default as ${c.name} } from './${c.name}';`).join('\n') || '// No protocol classes');

// Step 9: Create package.json
console.log('\n=== Step 9: Creating package.json ===');
const packageJson = {
    name: 'monsgeek-iot-driver-refactored',
    version: '1.0.0',
    description: 'Refactored MonsGeek/Epomaker IOT Driver',
    main: 'src/index.js',
    type: 'module',
    scripts: { start: 'electron .', build: 'webpack --mode production' },
    dependencies,
    devDependencies: { ...devDependencies, '@babel/core': '^7.23.0', webpack: '^5.89.0' },
    _extractionInfo: { extractedAt: new Date().toISOString(), sourceFile: inputFile },
};
fs.writeFileSync(path.join(dirs.root, 'package.json'), JSON.stringify(packageJson, null, 2));

// Main index
fs.writeFileSync(path.join(dirs.src, 'index.js'), `
// MonsGeek IOT Driver - Refactored
export * from './devices/index.js';
export * from './protocol/index.js';
export * from './components/index.js';
export * from './assets/svg/index.js';
`);

// Manifest
const manifest = {
    extractedAt: new Date().toISOString(),
    sourceFile: inputFile,
    sourceSize: code.length,
    statistics: {
        svgs: extracted.svgs.length,
        svgsTotalSize: extracted.svgs.reduce((a, b) => a + b.size, 0),
        css: extracted.css.length,
        components: extracted.components.length,
        hidClasses: extracted.classes.length,
        devices: devicesArray.length,
    },
    devices: {
        total: devicesArray.length,
        byType: devicesArray.reduce((acc, d) => { acc[d.type] = (acc[d.type] || 0) + 1; return acc; }, {}),
        uniqueVidPid: [...new Set(devicesArray.map(d => `${d.vidHex}:${d.pidHex}`))],
    },
};
fs.writeFileSync(path.join(dirs.root, 'manifest.json'), JSON.stringify(manifest, null, 2));

// Summary
console.log('\n' + '='.repeat(60));
console.log('EXTRACTION SUMMARY');
console.log('='.repeat(60));
console.log(`\nOutput: ${outputDir}`);
console.log(`\nExtracted:`);
console.log(`  • SVGs: ${extracted.svgs.length} (${(manifest.statistics.svgsTotalSize / 1024).toFixed(1)} KB)`);
console.log(`  • CSS: ${extracted.css.length}`);
console.log(`  • React components: ${extracted.components.length}`);
console.log(`  • HID classes: ${extracted.classes.length}`);
console.log(`  • Devices: ${devicesArray.length} (individual definitions)`);
console.log(`  • Device arrays: ${deviceArrays.length} (with original structure)`);

console.log(`\nDevices by type:`);
Object.entries(manifest.devices.byType).forEach(([t, c]) => console.log(`  • ${t}: ${c}`));

console.log(`\nUnique VID:PID:`);
manifest.devices.uniqueVidPid.slice(0, 15).forEach(v => console.log(`  • ${v}`));
if (manifest.devices.uniqueVidPid.length > 15) {
    console.log(`  ... and ${manifest.devices.uniqueVidPid.length - 15} more`);
}

console.log('\n' + '='.repeat(60));
