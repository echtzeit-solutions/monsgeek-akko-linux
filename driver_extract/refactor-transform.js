#!/usr/bin/env node
/**
 * MonsGeek/Epomaker IOT Driver Bundle Refactorer
 *
 * Uses Babel's visitor pattern to TRANSFORM the source:
 * - Extract content to separate module files
 * - Replace inline definitions with imports
 * - Babel generator handles all escaping
 *
 * Usage: node refactor-transform.js [input.js] [output-dir]
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
const SVG_SIZE_THRESHOLD = 500;
const CSS_SIZE_THRESHOLD = 200;

const inputFile = process.argv[2] || DEFAULT_INPUT;
const outputDir = process.argv[3] || DEFAULT_OUTPUT;

console.log('=== MonsGeek Driver Bundle Refactorer (Transform Mode) ===');
console.log(`Input: ${inputFile}`);
console.log(`Output: ${outputDir}`);

// Create output directories
const dirs = {
    root: outputDir,
    src: path.join(outputDir, 'src'),
    devices: path.join(outputDir, 'src/devices'),
    components: path.join(outputDir, 'src/components'),
    protocol: path.join(outputDir, 'src/protocol'),
    svg: path.join(outputDir, 'src/assets/svg'),
    css: path.join(outputDir, 'src/assets/css'),
};
Object.values(dirs).forEach(dir => fs.mkdirSync(dir, { recursive: true }));

// Read and parse
console.log('\nReading input file...');
const code = fs.readFileSync(inputFile, 'utf-8');
console.log(`File size: ${(code.length / 1024 / 1024).toFixed(2)} MB`);

console.log('\nParsing AST...');
const ast = parser.parse(code, {
    sourceType: 'module',
    plugins: ['jsx', 'typescript', 'classProperties', 'classPrivateProperties'],
    errorRecovery: true,
});

// State for extraction
const state = {
    extractions: [],      // { name, category, path, ast, deps }
    importsToAdd: [],     // { localName, importPath }
    variablesToRemove: new Set(), // variable names to remove from source
    stats: { svg: 0, css: 0, devices: 0, components: 0, protocol: 0 },
};

// Helper: Check if object looks like a device definition
function isDeviceObject(node) {
    if (!t.isObjectExpression(node)) return false;
    const props = new Set(node.properties
        .filter(p => t.isObjectProperty(p) && t.isIdentifier(p.key))
        .map(p => p.key.name));
    return props.has('id') && props.has('vid') && props.has('pid') && props.has('name') && props.has('type');
}

// Helper: Check if array contains device objects
function isDeviceArray(node) {
    if (!t.isArrayExpression(node)) return false;
    if (node.elements.length === 0) return false;
    return node.elements.some(el => isDeviceObject(el));
}

// Helper: Get string content from StringLiteral or TemplateLiteral
function getStringContent(node) {
    if (t.isStringLiteral(node)) return node.value;
    if (t.isTemplateLiteral(node)) {
        return node.quasis.map(q => q.value.raw).join('');
    }
    return null;
}

// Helper: Check if string is SVG
function isSvgString(str) {
    return str && str.includes('<svg') && str.length > SVG_SIZE_THRESHOLD;
}

// Helper: Check if this is a styled-components CSS
function isStyledCss(nodePath) {
    const parent = nodePath.parent;
    if (t.isTaggedTemplateExpression(parent)) {
        const tag = parent.tag;
        // styled.div`...` or css`...`
        if (t.isMemberExpression(tag) && tag.object?.name === 'styled') return true;
        if (t.isIdentifier(tag) && ['css', 'keyframes'].includes(tag.name)) return true;
        if (t.isCallExpression(tag) && t.isMemberExpression(tag.callee) && tag.callee.object?.name === 'styled') return true;
    }
    return false;
}

// Helper: Check if class is a React component
function isReactComponent(node) {
    const code = generate(node).code;
    const superClass = node.superClass?.name || node.superClass?.property?.name;
    return superClass?.includes('Component') ||
           code.includes('render()') ||
           code.includes('componentDidMount') ||
           code.includes('useState') ||
           code.includes('useEffect');
}

// Helper: Check if class is HID-related
function isHidClass(node) {
    const code = generate(node).code;
    return code.includes('sendReport') ||
           code.includes('navigator.hid') ||
           code.includes('sendFeatureReport') ||
           code.includes('receiveFeatureReport');
}

// Helper: Generate safe variable name
function safeName(name) {
    const reserved = ['default', 'class', 'function', 'return', 'if', 'else', 'for', 'while',
                      'switch', 'case', 'break', 'continue', 'var', 'let', 'const', 'import',
                      'export', 'new', 'this', 'delete', 'typeof', 'void', 'null', 'undefined',
                      'true', 'false', 'in', 'instanceof', 'try', 'catch', 'finally', 'throw'];
    let safe = name.replace(/[^a-zA-Z0-9_$]/g, '_');
    if (reserved.includes(safe) || /^\d/.test(safe)) safe = `_${safe}`;
    return safe;
}

// Helper: Create an export default statement for extracted content
function createExportDefault(name, valueNode) {
    return t.program([
        t.exportDefaultDeclaration(
            t.variableDeclaration('const', [
                t.variableDeclarator(t.identifier(name), valueNode)
            ]).declarations[0].init
        )
    ]);
}

// Helper: Create a module with export const and export default
function createModule(name, valueNode, deps = []) {
    const statements = [];

    // Add imports for dependencies
    deps.forEach(dep => {
        statements.push(
            t.importDeclaration(
                [t.importDefaultSpecifier(t.identifier(dep.localName))],
                t.stringLiteral(dep.importPath)
            )
        );
    });

    // Export const name = value
    statements.push(
        t.exportNamedDeclaration(
            t.variableDeclaration('const', [
                t.variableDeclarator(t.identifier(name), valueNode)
            ])
        )
    );

    // Export default name
    statements.push(
        t.exportDefaultDeclaration(t.identifier(name))
    );

    return t.program(statements);
}

console.log('\n=== Pass 1: Analyzing extractables ===');

// First pass: Identify what to extract
traverse(ast, {
    VariableDeclarator(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name) return;

        const init = nodePath.node.init;
        if (!init) return;

        // Device arrays (supportRY5088Dev, etc.)
        if (isDeviceArray(init)) {
            const safename = safeName(name);
            state.extractions.push({
                name: safename,
                originalName: name,
                category: 'devices',
                filePath: `./devices/${safename}.js`,
                relativePath: `./src/devices/${safename}.js`,
                ast: t.cloneNode(init, true),
                nodePath,
            });
            state.variablesToRemove.add(name);
            state.stats.devices++;
            console.log(`  Device array: ${name} (${init.elements.length} items)`);
        }

        // SVG strings
        const strContent = getStringContent(init);
        if (isSvgString(strContent)) {
            const safename = safeName(name) || `svg_${state.stats.svg}`;
            state.extractions.push({
                name: safename,
                originalName: name,
                category: 'svg',
                filePath: `./assets/svg/${safename}.js`,
                relativePath: `./src/assets/svg/${safename}.js`,
                ast: t.cloneNode(init, true),
                nodePath,
            });
            state.variablesToRemove.add(name);
            state.stats.svg++;
        }

        // KeyLayout, lightXX and other layout constants
        if (/^(KeyLayout|lightXX|sideLightXX)$/.test(name) && t.isObjectExpression(init)) {
            const safename = safeName(name);
            state.extractions.push({
                name: safename,
                originalName: name,
                category: 'devices',
                filePath: `./devices/${safename}.js`,
                relativePath: `./src/devices/${safename}.js`,
                ast: t.cloneNode(init, true),
                nodePath,
            });
            state.variablesToRemove.add(name);
            console.log(`  Layout constant: ${name}`);
        }
    },

    ClassDeclaration(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name) return;

        if (isHidClass(nodePath.node)) {
            const safename = safeName(name);
            state.extractions.push({
                name: safename,
                originalName: name,
                category: 'protocol',
                filePath: `./protocol/${safename}.js`,
                relativePath: `./src/protocol/${safename}.js`,
                ast: t.cloneNode(nodePath.node, true),
                nodePath,
                isClass: true,
            });
            state.variablesToRemove.add(name);
            state.stats.protocol++;
            console.log(`  HID class: ${name}`);
        } else if (isReactComponent(nodePath.node)) {
            const safename = safeName(name);
            state.extractions.push({
                name: safename,
                originalName: name,
                category: 'components',
                filePath: `./components/${safename}.jsx`,
                relativePath: `./src/components/${safename}.jsx`,
                ast: t.cloneNode(nodePath.node, true),
                nodePath,
                isClass: true,
            });
            state.variablesToRemove.add(name);
            state.stats.components++;
        }
    },
});

console.log(`\nFound: ${state.stats.devices} device arrays, ${state.stats.svg} SVGs, ${state.stats.components} components, ${state.stats.protocol} HID classes`);

console.log('\n=== Pass 2: Writing extracted modules ===');

// Write each extraction as a module
state.extractions.forEach(extraction => {
    const fullPath = path.join(outputDir, 'src', extraction.filePath.replace('./', ''));

    let moduleAst;
    if (extraction.isClass) {
        // Class: export default class Name { ... }
        moduleAst = t.program([
            t.exportDefaultDeclaration(extraction.ast)
        ]);
    } else {
        // Value: export const name = value; export default name;
        moduleAst = createModule(extraction.name, extraction.ast);
    }

    const output = generate(moduleAst, {
        comments: true,
        compact: false,
    });

    fs.writeFileSync(fullPath, `// Auto-extracted: ${extraction.originalName}\n${output.code}\n`);
});

console.log(`Wrote ${state.extractions.length} modules`);

console.log('\n=== Pass 3: Transforming source ===');

// Second pass: Transform the source - remove extracted variables, add imports
traverse(ast, {
    // Remove variable declarations that were extracted
    VariableDeclaration(nodePath) {
        const remaining = nodePath.node.declarations.filter(
            d => !state.variablesToRemove.has(d.id?.name)
        );

        if (remaining.length === 0) {
            nodePath.remove();
        } else if (remaining.length < nodePath.node.declarations.length) {
            nodePath.node.declarations = remaining;
        }
    },

    // Remove class declarations that were extracted
    ClassDeclaration(nodePath) {
        if (state.variablesToRemove.has(nodePath.node.id?.name)) {
            nodePath.remove();
        }
    },

    // Insert imports at the top of the program
    Program: {
        exit(nodePath) {
            // Build import statements for all extractions
            const imports = state.extractions.map(ext =>
                t.importDeclaration(
                    [t.importDefaultSpecifier(t.identifier(ext.name))],
                    t.stringLiteral(ext.filePath)
                )
            );

            // Insert at beginning
            nodePath.unshiftContainer('body', imports);
        }
    }
});

console.log('\n=== Pass 4: Generating output ===');

// Generate the transformed main file
const mainOutput = generate(ast, {
    comments: true,
    compact: false,
});

fs.writeFileSync(path.join(dirs.src, 'main.js'), mainOutput.code);
console.log(`Wrote main.js (${(mainOutput.code.length / 1024 / 1024).toFixed(2)} MB)`);

// Create index files
const deviceIndex = state.extractions
    .filter(e => e.category === 'devices')
    .map(e => `export { default as ${e.name} } from './${e.name}.js';`)
    .join('\n');
fs.writeFileSync(path.join(dirs.devices, 'index.js'), deviceIndex || '// No devices');

const componentIndex = state.extractions
    .filter(e => e.category === 'components')
    .map(e => `export { default as ${e.name} } from './${e.name}.jsx';`)
    .join('\n');
fs.writeFileSync(path.join(dirs.components, 'index.js'), componentIndex || '// No components');

const protocolIndex = state.extractions
    .filter(e => e.category === 'protocol')
    .map(e => `export { default as ${e.name} } from './${e.name}.js';`)
    .join('\n');
fs.writeFileSync(path.join(dirs.protocol, 'index.js'), protocolIndex || '// No protocol classes');

const svgIndex = state.extractions
    .filter(e => e.category === 'svg')
    .map(e => `export { default as ${e.name} } from './${e.name}.js';`)
    .join('\n');
fs.writeFileSync(path.join(dirs.svg, 'index.js'), svgIndex || '// No SVGs');

// Main index
fs.writeFileSync(path.join(dirs.src, 'index.js'), `
// MonsGeek IOT Driver - Refactored
export * from './devices/index.js';
export * from './components/index.js';
export * from './protocol/index.js';
export * from './assets/svg/index.js';
`);

// Package.json
const packageJson = {
    name: 'monsgeek-iot-driver-refactored',
    version: '1.0.0',
    type: 'module',
    main: 'src/index.js',
    dependencies: {
        'react': '^18.2.0',
        'react-dom': '^18.2.0',
    },
    _extractionInfo: {
        extractedAt: new Date().toISOString(),
        sourceFile: inputFile,
    }
};
fs.writeFileSync(path.join(dirs.root, 'package.json'), JSON.stringify(packageJson, null, 2));

console.log('\n' + '='.repeat(60));
console.log('EXTRACTION COMPLETE');
console.log('='.repeat(60));
console.log(`\nExtracted modules:`);
console.log(`  • Device arrays: ${state.stats.devices}`);
console.log(`  • SVGs: ${state.stats.svg}`);
console.log(`  • React components: ${state.stats.components}`);
console.log(`  • HID/Protocol: ${state.stats.protocol}`);
console.log(`\nOutput: ${outputDir}`);
console.log('='.repeat(60));
