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

// Clean output directory
if (fs.existsSync(outputDir)) {
    fs.rmSync(outputDir, { recursive: true, force: true });
    console.log('Cleaned previous output');
}

// Create output directories
const dirs = {
    root: outputDir,
    src: path.join(outputDir, 'src'),
    devices: path.join(outputDir, 'src/devices'),
    components: path.join(outputDir, 'src/components'),
    protocol: path.join(outputDir, 'src/protocol'),
    protobuf: path.join(outputDir, 'src/protobuf'),
    state: path.join(outputDir, 'src/state'),
    utils: path.join(outputDir, 'src/utils'),
    svg: path.join(outputDir, 'src/assets/svg'),
    css: path.join(outputDir, 'src/assets/css'),
    data: path.join(outputDir, 'src/assets/data'),
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
    vendorModules: new Set(),     // vendor module names to track
    stats: { svg: 0, css: 0, devices: 0, components: 0, protocol: 0, protobuf: 0, state: 0, utils: 0, vendor: 0 },
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
           superClass?.includes('PureComponent') ||
           code.includes('render()') ||
           code.includes('componentDidMount') ||
           code.includes('useState') ||
           code.includes('useEffect');
}

// Helper: Check if class is protobuf/grpc related
function isProtobufClass(node) {
    const name = node.id?.name || '';
    const code = generate(node).code.slice(0, 500);
    return /^(Pb|Shared|Binary|Reflection|Message|Service|Rpc|Unary|Server|Client)/.test(name) ||
           code.includes('BinaryReader') ||
           code.includes('BinaryWriter') ||
           code.includes('grpc');
}

// Helper: Check if variable is a vendor module (should be externalized)
function isVendorModule(name, node) {
    // Known vendor variable name patterns
    const vendorPatterns = [
        /^react/, /^React/, /^scheduler/, /^emotion/, /^styled/,
        /^axios/, /^Axios/, /^dayjs/, /^lodash/, /^mobx/, /^zustand/,
        /_production_min$/, /Exports$/, /__esModule/,
        /^jsxRuntime/, /^jsxDEV/, /^jsx$/, /^jsxs$/,
        /^createRoot$/, /^ReactDOM$/,
        // React internal symbols
        /^l\$\d+$/, /^n\$\d+$/, /^p\$\d+$/, /^q\$\d+$/, /^r\$\d+$/, /^t\$\d+$/,
        /^u\$\d+$/, /^w\$?\d*$/, /^x\$\d+$/, /^y\$\d+$/, /^z\$\d+$/,
        // Other vendor patterns
        /^encoder$/, /^decoder$/, /^JpegImage$/, /^module\$\d*$/,
        /^gif$/, /^GIF$/, /^huffman/i,
        // Axios patterns
        /^AxiosError$/, /^AxiosURLSearchParams$/, /^AxiosHeaders$/,
        /^CanceledError$/, /^CancelToken$/, /^isCancel$/,
        // Buffer/pako patterns
        /^Buffer$/, /^SlowBuffer$/, /^pako/, /^zlibjs$/,
        /^ImageService$/,
    ];
    if (vendorPatterns.some(p => p.test(name))) return true;

    // Check for vendor markers in code
    if (node) {
        const code = generate(node).code.slice(0, 500);
        if (code.includes('__esModule') ||
            code.includes('production_min') ||
            code.includes('getDefaultExportFromCjs') ||
            code.includes('Symbol.for("react') ||
            code.includes('huffman') ||
            code.includes('jpeg') ||
            code.includes('JPEG') ||
            code.includes('pako 2.1') ||
            code.includes('buffer module from node.js') ||
            code.includes('@author   Feross') ||
            code.includes('@license  MIT') ||
            code.includes('axios')) {
            return true;
        }
    }
    return false;
}

// Helper: Check if this is a CommonJS wrapper pattern: var x = { exports: {} };
function isCommonJSWrapper(node) {
    if (!t.isObjectExpression(node)) return false;
    const props = node.properties;
    if (props.length !== 1) return false;
    const prop = props[0];
    return t.isObjectProperty(prop) &&
           t.isIdentifier(prop.key, { name: 'exports' }) &&
           t.isObjectExpression(prop.value) &&
           prop.value.properties.length === 0;
}

// Helper: Check if node has vendor license comment
function hasVendorLicenseComment(nodePath) {
    const comments = nodePath.node.leadingComments;
    if (!comments) return false;
    for (const comment of comments) {
        const text = comment.value;
        if (/@license\s*(React|scheduler|emotion|dayjs|axios|lodash)/i.test(text) ||
            /production\.min\.js/i.test(text) ||
            /Copyright.*Facebook/i.test(text)) {
            return true;
        }
    }
    return false;
}

// Helper: Check if it's a keyboard protocol class
function isKeyboardProtocolClass(node) {
    const name = node.id?.name || '';
    return /^(Common|Keyboard|Mouse|Audio)/.test(name) &&
           /(KB|MS|YC|RY|Yzw|Pan|Ch)/.test(name);
}

// Helper: Check if class is HID-related
function isHidClass(node) {
    const code = generate(node).code;
    return code.includes('sendReport') ||
           code.includes('navigator.hid') ||
           code.includes('sendFeatureReport') ||
           code.includes('receiveFeatureReport');
}

// Track used safe names to avoid duplicates
const usedSafeNames = new Set();

// Helper: Generate safe variable name
function safeName(name) {
    const reserved = ['default', 'class', 'function', 'return', 'if', 'else', 'for', 'while',
                      'switch', 'case', 'break', 'continue', 'var', 'let', 'const', 'import',
                      'export', 'new', 'this', 'delete', 'typeof', 'void', 'null', 'undefined',
                      'true', 'false', 'in', 'instanceof', 'try', 'catch', 'finally', 'throw',
                      // Short names that are commonly used as local variables
                      'Ft', 'Rt', 'St', 'Zt', 'Kt', 'Ut', 'Ht', 'Bt', 'Dt', 'Jt', 'Yt',
                      'g', 'd', 'et', 'ft', 'ct', 'ut', 'dt', 'kt', 'ot', 'zt'];
    let safe = name.replace(/[^a-zA-Z0-9_$]/g, '_');
    if (reserved.includes(safe) || /^\d/.test(safe)) safe = `_${safe}`;

    // Handle duplicates by adding a counter
    let finalName = safe;
    let counter = 1;
    while (usedSafeNames.has(finalName)) {
        finalName = `${safe}_${counter}`;
        counter++;
    }
    usedSafeNames.add(finalName);
    return finalName;
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

console.log('\n=== Pass 1a: Detecting vendor/CommonJS wrappers ===');

// First, collect all CommonJS wrappers (var x = { exports: {} })
traverse(ast, {
    VariableDeclarator(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name) return;
        const init = nodePath.node.init;
        if (!init) return;

        // Track vendor modules by name pattern
        if (isVendorModule(name, init)) {
            state.vendorModules.add(name);
            state.variablesToRemove.add(name);
            state.stats.vendor++;
            return;
        }

        // Track CommonJS wrapper pattern: var x = { exports: {} };
        if (isCommonJSWrapper(init)) {
            state.vendorModules.add(name);
            state.variablesToRemove.add(name);
            state.stats.vendor++;
            return;
        }
    }
});

console.log(`  Found ${state.vendorModules.size} CommonJS/vendor wrappers`);

console.log('\n=== Pass 1b: Removing vendor IIFEs ===');

// Now remove IIFEs that use the detected wrappers
let iifeCount = 0;
traverse(ast, {
    ExpressionStatement(nodePath) {
        const expr = nodePath.node.expression;
        if (!t.isCallExpression(expr)) return;

        const callee = expr.callee;
        // Check if it's an IIFE: (function(...){...})(args)
        if (!t.isFunctionExpression(callee) && !t.isArrowFunctionExpression(callee)) return;

        // Check if any argument is a known vendor wrapper
        const args = expr.arguments;
        for (const arg of args) {
            if (t.isIdentifier(arg) && state.vendorModules.has(arg.name)) {
                nodePath.remove();
                iifeCount++;
                return;
            }
        }
    }
});

console.log(`  Removed ${iifeCount} vendor IIFEs`);

console.log('\n=== Pass 1b2: Removing vendor property assignments ===');

// Remove property assignments to vendor modules: vendorModule.prop = ...
let propAssignCount = 0;
traverse(ast, {
    ExpressionStatement(nodePath) {
        const expr = nodePath.node.expression;
        if (!t.isAssignmentExpression(expr)) return;

        const left = expr.left;
        if (!t.isMemberExpression(left)) return;

        // Check if assigning to a vendor module property
        if (t.isIdentifier(left.object) && state.vendorModules.has(left.object.name)) {
            nodePath.remove();
            propAssignCount++;
        }
    }
});

console.log(`  Removed ${propAssignCount} vendor property assignments`);

console.log('\n=== Pass 1b3: Removing vendor functions ===');

// Remove functions that match vendor patterns by name or have vendor license comments
let vendorFuncCount = 0;
traverse(ast, {
    FunctionDeclaration(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name) return;

        // Check name pattern
        if (/^Axios|^Buffer|^SlowBuffer|^pako|^inflate|^deflate/i.test(name)) {
            nodePath.remove();
            vendorFuncCount++;
            return;
        }

        // Check for vendor license comments
        const comments = nodePath.node.leadingComments || [];
        for (const comment of comments) {
            if (comment.value.includes('@license') ||
                comment.value.includes('buffer module from node.js') ||
                comment.value.includes('pako') ||
                comment.value.includes('axios') ||
                comment.value.includes('Feross')) {
                nodePath.remove();
                vendorFuncCount++;
                return;
            }
        }
    },

    // Also remove IIFEs with vendor comments
    ExpressionStatement(nodePath) {
        const expr = nodePath.node.expression;
        if (!t.isCallExpression(expr)) return;

        const comments = nodePath.node.leadingComments || [];
        for (const comment of comments) {
            if (comment.value.includes('@license') ||
                comment.value.includes('buffer module from node.js') ||
                comment.value.includes('pako') ||
                comment.value.includes('axios') ||
                comment.value.includes('Feross')) {
                nodePath.remove();
                vendorFuncCount++;
                return;
            }
        }
    }
});

console.log(`  Removed ${vendorFuncCount} vendor functions`);

console.log('\n=== Pass 1c: Analyzing extractables ===');

// Now identify what to extract
traverse(ast, {
    VariableDeclarator(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name) return;

        const init = nodePath.node.init;
        if (!init) return;

        // Skip already-detected vendor modules
        if (state.vendorModules.has(name)) return;

        // Device arrays (supportRY5088Dev, etc.)
        if (isDeviceArray(init)) {
            const safename = safeName(name);
            const code = generate(init).code;
            // Detect dependencies
            const deps = [];
            if (code.includes('KeyLayout.')) {
                deps.push({ localName: 'KeyLayout', importPath: './KeyLayout.js' });
            }
            if (code.includes('HidMapping.')) {
                deps.push({ localName: 'HidMapping', importPath: './HidMapping.js' });
            }
            state.extractions.push({
                name: safename,
                originalName: name,
                category: 'devices',
                filePath: `./devices/${safename}.js`,
                relativePath: `./src/devices/${safename}.js`,
                ast: t.cloneNode(init, true),
                nodePath,
                deps,
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
        // KeyLayout is an IIFE: var KeyLayout = (g => { g.X = "..."; return g; })({})
        // lightXX/sideLightXX are objects
        if (/^(KeyLayout|lightXX|sideLightXX)$/.test(name)) {
            if (t.isObjectExpression(init) || t.isCallExpression(init)) {
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
        }

        // Large JSON data blobs (figma_json, strings_json, etc.)
        // Skip extraction if they have external dependencies (identifiers)
        if (/^(figma|strings|i18n|lang).*json/i.test(name) || /^locale/i.test(name)) {
            const code = generate(init).code;
            // Check for external references (identifiers that aren't object keys)
            // If the code contains identifiers like _xxx_str, it has dependencies
            const hasExternalRefs = /_\w+_str\b/.test(code);
            if (code.length > 50000 && !hasExternalRefs) {
                const safename = safeName(name);
                state.extractions.push({
                    name: safename,
                    originalName: name,
                    category: 'assets',
                    filePath: `./assets/data/${safename}.js`,
                    relativePath: `./src/assets/data/${safename}.js`,
                    ast: t.cloneNode(init, true),
                    nodePath,
                });
                state.variablesToRemove.add(name);
                state.stats.svg++; // Reuse svg counter for large data
                console.log(`  Large data: ${name} (${(code.length/1024).toFixed(0)}KB)`);
            } else if (hasExternalRefs) {
                console.log(`  Skipping ${name} (has external dependencies)`);
            }
        }

        // Large icon/image data
        if (/Icon\$?\d*$/.test(name) && (t.isStringLiteral(init) || t.isTemplateLiteral(init))) {
            const content = getStringContent(init);
            if (content && content.length > 50000) {
                const safename = safeName(name);
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
                console.log(`  Large icon: ${name} (${(content.length/1024).toFixed(0)}KB)`);
            }
        }

        // Zlib/inflate utilities (trees, inflate, etc.)
        if (/^(inflate|trees|huffman|zlib)/i.test(name)) {
            const code = generate(init).code;
            if (code.length > 5000) {
                const safename = safeName(name);
                state.extractions.push({
                    name: safename,
                    originalName: name,
                    category: 'utils',
                    filePath: `./utils/${safename}.js`,
                    relativePath: `./src/utils/${safename}.js`,
                    ast: t.cloneNode(init, true),
                    nodePath,
                });
                state.variablesToRemove.add(name);
                state.stats.utils++;
                console.log(`  Zlib util: ${name}`);
            }
        }

        // Object-wrapped SVGs (const foo = { bar: "<svg>..." })
        if (t.isObjectExpression(init)) {
            const code = generate(init).code;
            if (code.includes('<svg') && code.length > 1000) {
                const safename = safeName(name);
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
        }

        // GIF/Image utilities
        if (/^(Gif|GIF|WuQuant|_WuQuant)/i.test(name)) {
            const code = generate(init).code;
            if (code.length > 5000) {
                const safename = safeName(name);
                state.extractions.push({
                    name: safename,
                    originalName: name,
                    category: 'utils',
                    filePath: `./utils/${safename}.js`,
                    relativePath: `./src/utils/${safename}.js`,
                    ast: t.cloneNode(init, true),
                    nodePath,
                });
                state.variablesToRemove.add(name);
                state.stats.utils++;
                console.log(`  Image util: ${name}`);
            }
        }
    },

    // Large require-style wrapper functions (function requireXXX() { ... })
    FunctionDeclaration(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name) return;

        // Detect CommonJS-style require wrappers
        if (/^require/.test(name)) {
            const code = generate(nodePath.node).code;
            if (code.length > 3000) {
                const safename = safeName(name);
                state.extractions.push({
                    name: safename,
                    originalName: name,
                    category: 'utils',
                    filePath: `./utils/${safename}.js`,
                    relativePath: `./src/utils/${safename}.js`,
                    ast: t.cloneNode(nodePath.node, true),
                    nodePath,
                    isFunc: true,
                });
                state.variablesToRemove.add(name);
                state.stats.utils++;
                console.log(`  Require wrapper: ${name} (${(code.length/1024).toFixed(0)}KB)`);
            }
        }
    },

    ClassDeclaration(nodePath) {
        const name = nodePath.node.id?.name;
        if (!name) return;

        // Skip small/internal classes
        const code = generate(nodePath.node).code;
        if (code.length < 200) return;

        let category = null;
        let dir = null;
        let ext = '.js';

        if (isHidClass(nodePath.node)) {
            category = 'protocol';
            dir = 'protocol';
            console.log(`  HID class: ${name}`);
        } else if (isKeyboardProtocolClass(nodePath.node)) {
            category = 'protocol';
            dir = 'protocol';
            console.log(`  Keyboard protocol: ${name}`);
        } else if (isProtobufClass(nodePath.node)) {
            category = 'protobuf';
            dir = 'protobuf';
            console.log(`  Protobuf class: ${name}`);
        } else if (isReactComponent(nodePath.node)) {
            category = 'components';
            dir = 'components';
            ext = '.jsx';
        } else if (/State|Store|Manager|Context/.test(name)) {
            category = 'state';
            dir = 'state';
            console.log(`  State class: ${name}`);
        } else if (code.length > 1000) {
            // Extract large utility classes
            category = 'utils';
            dir = 'utils';
        }

        if (category) {
            const safename = safeName(name);
            state.extractions.push({
                name: safename,
                originalName: name,
                category,
                filePath: `./${dir}/${safename}${ext}`,
                relativePath: `./src/${dir}/${safename}${ext}`,
                ast: t.cloneNode(nodePath.node, true),
                nodePath,
                isClass: true,
            });
            state.variablesToRemove.add(name);
            state.stats[category]++;
        }
    },
});

console.log(`\nFound: ${state.stats.devices} devices, ${state.stats.svg} SVGs, ${state.stats.components} components, ${state.stats.protocol} protocol, ${state.stats.protobuf} protobuf, ${state.stats.utils} utils, ${state.stats.vendor} vendor (removed)`);

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
    } else if (extraction.isFunc) {
        // Function: export default function name() { ... }
        moduleAst = t.program([
            t.exportDefaultDeclaration(extraction.ast)
        ]);
    } else {
        // Value: export const name = value; export default name;
        moduleAst = createModule(extraction.name, extraction.ast, extraction.deps || []);
    }

    const output = generate(moduleAst, {
        comments: true,
        compact: false,
    });

    fs.writeFileSync(fullPath, `// Auto-extracted: ${extraction.originalName}\n${output.code}\n`);
});

console.log(`Wrote ${state.extractions.length} modules`);

console.log('\n=== Pass 3: Transforming source ===');

// Track if we need JSX runtime import
state.needsJsxRuntime = false;
state.needsReact = false;
state.reactHooks = new Set();

// Second pass: Transform the source - remove extracted variables, add imports
traverse(ast, {
    // Replace jsxRuntimeExports.xxx with direct calls
    MemberExpression(nodePath) {
        const obj = nodePath.node.object;
        const prop = nodePath.node.property;

        // Handle jsxRuntimeExports.xxx
        if (t.isIdentifier(obj, { name: 'jsxRuntimeExports' }) && t.isIdentifier(prop)) {
            const name = prop.name;
            if (['jsx', 'jsxs', 'Fragment'].includes(name)) {
                nodePath.replaceWith(t.identifier(name));
                state.needsJsxRuntime = true;
            }
        }

        // Handle reactExports.xxx -> React.xxx or direct hook import
        if (t.isIdentifier(obj, { name: 'reactExports' }) && t.isIdentifier(prop)) {
            const name = prop.name;
            state.needsReact = true;
            // Track hooks for named imports
            if (/^use[A-Z]|^create|^is[A-Z]|^clone|^Children|^lazy|^memo|^forwardRef|^Suspense|^Fragment/.test(name)) {
                state.reactHooks.add(name);
            }
            // Replace with React.xxx
            nodePath.replaceWith(t.memberExpression(t.identifier('React'), t.identifier(name)));
        }
    },

    // Remove variable declarations that were extracted
    VariableDeclaration(nodePath) {
        // Skip if part of for-in/for-of loop
        if (nodePath.parentPath.isForInStatement() || nodePath.parentPath.isForOfStatement()) {
            return;
        }
        // Skip if not at statement level (e.g., in a for loop init)
        if (nodePath.parentPath.isForStatement()) {
            return;
        }

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

    // Remove function declarations that were extracted
    FunctionDeclaration(nodePath) {
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

            // Add JSX runtime imports for removed vendor modules
            const runtimeImports = [];

            // Add React import if needed
            if (state.needsReact) {
                runtimeImports.push(
                    t.importDeclaration(
                        [t.importNamespaceSpecifier(t.identifier('React'))],
                        t.stringLiteral('react')
                    )
                );
            }

            if (state.needsJsxRuntime) {
                runtimeImports.push(
                    t.importDeclaration(
                        [
                            t.importSpecifier(t.identifier('jsx'), t.identifier('jsx')),
                            t.importSpecifier(t.identifier('jsxs'), t.identifier('jsxs')),
                            t.importSpecifier(t.identifier('Fragment'), t.identifier('Fragment')),
                        ],
                        t.stringLiteral('react/jsx-runtime')
                    )
                );
            }

            // Insert at beginning
            nodePath.unshiftContainer('body', [...runtimeImports, ...imports]);
        }
    }
});

console.log('\n=== Pass 4: Generating output ===');

// Generate the transformed main file
const mainOutput = generate(ast, {
    comments: true,
    compact: false,
});

fs.writeFileSync(path.join(dirs.src, 'main.jsx'), mainOutput.code);
console.log(`Wrote main.jsx (${(mainOutput.code.length / 1024 / 1024).toFixed(2)} MB)`);

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

const protobufIndex = state.extractions
    .filter(e => e.category === 'protobuf')
    .map(e => `export { default as ${e.name} } from './${e.name}.js';`)
    .join('\n');
fs.writeFileSync(path.join(dirs.protobuf, 'index.js'), protobufIndex || '// No protobuf classes');

const stateIndex = state.extractions
    .filter(e => e.category === 'state')
    .map(e => `export { default as ${e.name} } from './${e.name}.js';`)
    .join('\n');
fs.writeFileSync(path.join(dirs.state, 'index.js'), stateIndex || '// No state classes');

const utilsIndex = state.extractions
    .filter(e => e.category === 'utils')
    .map(e => `export { default as ${e.name} } from './${e.name}.js';`)
    .join('\n');
fs.writeFileSync(path.join(dirs.utils, 'index.js'), utilsIndex || '// No utility classes');

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
export * from './protobuf/index.js';
export * from './state/index.js';
export * from './utils/index.js';
export * from './assets/svg/index.js';
`);

// Package.json
const packageJson = {
    name: 'monsgeek-iot-driver-refactored',
    version: '1.0.0',
    type: 'module',
    main: 'src/index.js',
    scripts: {
        dev: 'vite',
        build: 'vite build',
        preview: 'vite preview',
    },
    dependencies: {
        'react': '^18.2.0',
        'react-dom': '^18.2.0',
    },
    devDependencies: {
        '@vitejs/plugin-react': '^4.2.0',
        'vite': '^5.0.0',
    },
    _extractionInfo: {
        extractedAt: new Date().toISOString(),
        sourceFile: inputFile,
    }
};
fs.writeFileSync(path.join(dirs.root, 'package.json'), JSON.stringify(packageJson, null, 2));

// Also write vite.config.js and index.html
const viteConfig = `import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [
    react({
      babel: {
        generatorOpts: {
          compact: false,
          retainLines: true,
        },
      },
    }),
  ],
  server: {
    port: 3000,
  },
  optimizeDeps: {
    include: ['react', 'react-dom'],
  },
});
`;
fs.writeFileSync(path.join(dirs.root, 'vite.config.js'), viteConfig);

const indexHtml = `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>MonsGeek IOT Driver</title>
    <style>
      body { margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }
    </style>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.jsx"></script>
  </body>
</html>
`;
fs.writeFileSync(path.join(dirs.root, 'index.html'), indexHtml);

console.log('\n' + '='.repeat(60));
console.log('EXTRACTION COMPLETE');
console.log('='.repeat(60));
console.log(`\nExtracted modules:`);
console.log(`  • Device arrays: ${state.stats.devices}`);
console.log(`  • SVGs: ${state.stats.svg}`);
console.log(`  • React components: ${state.stats.components}`);
console.log(`  • Protocol classes: ${state.stats.protocol}`);
console.log(`  • Protobuf classes: ${state.stats.protobuf}`);
console.log(`  • State classes: ${state.stats.state}`);
console.log(`  • Utility classes: ${state.stats.utils}`);
console.log(`  • Total: ${state.extractions.length}`);
console.log(`\nOutput: ${outputDir}`);
console.log('='.repeat(60));
