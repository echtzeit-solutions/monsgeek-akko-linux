#!/usr/bin/env node

/**
 * Babel transform to add stubs for undefined references in extracted device files.
 *
 * Detects undefined identifiers and adds appropriate stub declarations:
 * - KeyLayout → Proxy that returns property names
 * - HidMapping → Object with htmlCodeMapHIDCode passthrough
 * - light*, sideLight* → String constants
 */

import { parse } from '@babel/parser';
import traverse from '@babel/traverse';
import generate from '@babel/generator';
import * as t from '@babel/types';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Stub generators for different patterns
const stubGenerators = {
  KeyLayout: () => t.variableDeclaration('const', [
    t.variableDeclarator(
      t.identifier('KeyLayout'),
      t.newExpression(t.identifier('Proxy'), [
        t.objectExpression([]),
        t.objectExpression([
          t.objectProperty(
            t.identifier('get'),
            t.arrowFunctionExpression(
              [t.identifier('t'), t.identifier('p')],
              t.identifier('p')
            )
          )
        ])
      ])
    )
  ]),

  HidMapping: () => t.variableDeclaration('const', [
    t.variableDeclarator(
      t.identifier('HidMapping'),
      t.objectExpression([
        t.objectProperty(
          t.identifier('htmlCodeMapHIDCode'),
          t.arrowFunctionExpression(
            [t.identifier('code')],
            t.identifier('code')
          )
        )
      ])
    )
  ]),

  currentCompany: () => t.variableDeclaration('const', [
    t.variableDeclarator(
      t.identifier('currentCompany'),
      t.objectExpression([
        t.objectProperty(
          t.identifier('currentCompany'),
          t.stringLiteral('default')
        )
      ])
    )
  ]),

  // Generic string constant stub
  stringConstant: (name) => t.variableDeclaration('const', [
    t.variableDeclarator(
      t.identifier(name),
      t.stringLiteral(name)
    )
  ])
};

// Patterns for stub generation
const stubPatterns = [
  { match: /^KeyLayout$/, generator: 'KeyLayout' },
  { match: /^HidMapping$/, generator: 'HidMapping' },
  { match: /^currentCompany$/, generator: 'currentCompany' },
  { match: /^light/, generator: 'stringConstant' },
  { match: /^sideLight/, generator: 'stringConstant' },
];

function findUndefinedReferences(code) {
  const ast = parse(code, {
    sourceType: 'module',
    plugins: ['jsx']
  });

  const defined = new Set();
  const referenced = new Set();

  // First pass: collect all definitions (including function parameters)
  traverse.default(ast, {
    VariableDeclarator(nodePath) {
      if (t.isIdentifier(nodePath.node.id)) {
        defined.add(nodePath.node.id.name);
      }
    },
    FunctionDeclaration(nodePath) {
      if (nodePath.node.id) {
        defined.add(nodePath.node.id.name);
      }
      // Add function parameters
      for (const param of nodePath.node.params) {
        if (t.isIdentifier(param)) {
          defined.add(param.name);
        }
      }
    },
    FunctionExpression(nodePath) {
      for (const param of nodePath.node.params) {
        if (t.isIdentifier(param)) {
          defined.add(param.name);
        }
      }
    },
    ArrowFunctionExpression(nodePath) {
      for (const param of nodePath.node.params) {
        if (t.isIdentifier(param)) {
          defined.add(param.name);
        }
      }
    },
    ClassDeclaration(nodePath) {
      if (nodePath.node.id) {
        defined.add(nodePath.node.id.name);
      }
    },
    ImportSpecifier(nodePath) {
      defined.add(nodePath.node.local.name);
    },
    ImportDefaultSpecifier(nodePath) {
      defined.add(nodePath.node.local.name);
    },
    ImportNamespaceSpecifier(nodePath) {
      defined.add(nodePath.node.local.name);
    }
  });

  // Second pass: collect all references
  traverse.default(ast, {
    Identifier(nodePath) {
      const name = nodePath.node.name;

      // Skip if it's a definition, property access key, or standard globals
      if (nodePath.isBindingIdentifier()) return;
      if (nodePath.parentPath.isMemberExpression() &&
          nodePath.parentPath.node.property === nodePath.node &&
          !nodePath.parentPath.node.computed) return;
      if (nodePath.parentPath.isObjectProperty() &&
          nodePath.parentPath.node.key === nodePath.node) return;

      // Skip standard globals
      const globals = ['undefined', 'null', 'true', 'false', 'console',
                       'Array', 'Object', 'String', 'Number', 'Boolean',
                       'Promise', 'Map', 'Set', 'JSON', 'Math', 'Date',
                       'Error', 'TypeError', 'ReferenceError', 'Proxy',
                       'Reflect', 'Symbol', 'parseInt', 'parseFloat',
                       'setTimeout', 'setInterval', 'clearTimeout',
                       'window', 'document', 'navigator', 'fetch',
                       'exports', 'module', 'require', '__dirname', '__filename'];
      if (globals.includes(name)) return;

      referenced.add(name);
    },
    MemberExpression(nodePath) {
      // Handle KeyLayout.Something or HidMapping.something
      if (t.isIdentifier(nodePath.node.object)) {
        referenced.add(nodePath.node.object.name);
      }
    }
  });

  // Find undefined references
  const undefined_ = [];
  for (const ref of referenced) {
    if (!defined.has(ref)) {
      undefined_.push(ref);
    }
  }

  return { ast, undefined: undefined_.sort() };
}

function generateStubs(undefinedRefs) {
  const stubs = [];
  const handled = new Set();

  for (const ref of undefinedRefs) {
    if (handled.has(ref)) continue;

    for (const pattern of stubPatterns) {
      if (pattern.match.test(ref)) {
        const generator = stubGenerators[pattern.generator];
        if (generator) {
          if (pattern.generator === 'stringConstant') {
            stubs.push(generator(ref));
          } else {
            stubs.push(generator());
          }
          handled.add(ref);
        }
        break;
      }
    }
  }

  return stubs;
}

function transformFile(filePath) {
  const code = fs.readFileSync(filePath, 'utf-8');

  // Remove existing stubs comment block if present
  const cleanedCode = code.replace(
    /\/\/ Stubs for referenced constants\n(const \w+ = [^;]+;\n)*/g,
    ''
  );

  const { ast, undefined: undefinedRefs } = findUndefinedReferences(cleanedCode);

  if (undefinedRefs.length === 0) {
    console.log(`  No undefined references in ${path.basename(filePath)}`);
    return null;
  }

  console.log(`  Found undefined refs in ${path.basename(filePath)}: ${undefinedRefs.join(', ')}`);

  const stubs = generateStubs(undefinedRefs);

  if (stubs.length === 0) {
    console.log(`  No matching stub patterns for: ${undefinedRefs.join(', ')}`);
    return null;
  }

  // Re-parse clean code and add stubs
  const cleanAst = parse(cleanedCode, {
    sourceType: 'module',
    plugins: ['jsx']
  });

  // Find insertion point (after any leading comments, before first statement)
  // Add comment before stubs
  const stubComment = t.addComment(
    stubs[0],
    'leading',
    ' Stubs for referenced constants (auto-generated)',
    true
  );

  // Insert stubs at the beginning of the program body
  // But after any existing imports
  let insertIndex = 0;
  for (let i = 0; i < cleanAst.program.body.length; i++) {
    if (t.isImportDeclaration(cleanAst.program.body[i])) {
      insertIndex = i + 1;
    } else {
      break;
    }
  }

  cleanAst.program.body.splice(insertIndex, 0, ...stubs);

  const output = generate.default(cleanAst, {
    comments: true,
    compact: false
  });

  return output.code;
}

function processDevicesDirectory(devicesDir) {
  console.log(`\nProcessing devices in: ${devicesDir}\n`);

  const files = fs.readdirSync(devicesDir)
    .filter(f => f.endsWith('.js') && !f.startsWith('index'));

  let modified = 0;

  for (const file of files) {
    const filePath = path.join(devicesDir, file);
    const result = transformFile(filePath);

    if (result) {
      fs.writeFileSync(filePath, result);
      console.log(`  ✓ Updated ${file}`);
      modified++;
    }
  }

  console.log(`\nModified ${modified} files`);
}

// Main
const devicesDir = process.argv[2] || path.join(__dirname, 'refactored-v2/src/devices');
processDevicesDirectory(devicesDir);
