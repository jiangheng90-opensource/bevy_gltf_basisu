#!/usr/bin/env node

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

if (process.argv.length < 3) {
    console.error('Usage: node tools/wasm_build.js <example_name> [--sync-assets] [features...]');
    console.error('Example: node tools/wasm_build.js example --sync-assets webgpu debug');
    console.error('Example: node tools/wasm_build.js example webgpu debug (do not sync assets)');
    process.exit(1);
}

const EXAMPLE_NAME = process.argv[2];
let SYNC_ASSETS = false;
const FEATURES = [];

// Parse arguments
for (let i = 3; i < process.argv.length; i++) {
    const arg = process.argv[i];
    if (arg === '--sync-assets') {
        SYNC_ASSETS = true;
    } else {
        FEATURES.push(arg);
    }
}

const OUTPUT_DIR = 'examples/wasm/target';
const TARGET_PLATFORM = 'wasm32-unknown-unknown';
const WASM_BINDGEN_TARGET = 'web';

// Create output directory
if (!fs.existsSync(OUTPUT_DIR)) {
    fs.mkdirSync(OUTPUT_DIR, { recursive: true });
}

console.log(`Building example '${EXAMPLE_NAME}' for target '${TARGET_PLATFORM}'...`);

// Build cargo command
const BUILD_ARGS = ['--target', TARGET_PLATFORM, '--example', EXAMPLE_NAME, '--release'];

if (FEATURES.length > 0) {
    console.log(`Enabling Cargo features: ${FEATURES.join(' ')}`);
    FEATURES.forEach(feature => {
        BUILD_ARGS.push('--features', feature);
    });
}

try {
    execSync(`cargo build ${BUILD_ARGS.join(' ')}`, { stdio: 'inherit' });
} catch (error) {
    console.error(`Build failed for example '${EXAMPLE_NAME}'.`);
    process.exit(1);
}

const WASM_FILE = `target/${TARGET_PLATFORM}/release/examples/${EXAMPLE_NAME}.wasm`;
const CUSTOM_OUTPUT_NAME = 'wasm_example';

console.log('Processing wasm file with wasm-bindgen...');

try {
    execSync(
        `wasm-bindgen "${WASM_FILE}" --out-dir "${OUTPUT_DIR}" --out-name "${CUSTOM_OUTPUT_NAME}" --target "${WASM_BINDGEN_TARGET}"`,
        { stdio: 'inherit' }
    );
} catch (error) {
    console.error(`wasm-bindgen processing failed for '${EXAMPLE_NAME}'.`);
    process.exit(1);
}

if (SYNC_ASSETS) {
    const ASSETS_SOURCE_DIR = 'assets';
    const ASSETS_TARGET_DIR = 'examples/wasm/assets';

    console.log('Syncing assets to wasm output directory...');

    // Remove existing assets directory
    if (fs.existsSync(ASSETS_TARGET_DIR)) {
        fs.rmSync(ASSETS_TARGET_DIR, { recursive: true, force: true });
    }

    // Create target directory
    fs.mkdirSync(ASSETS_TARGET_DIR, { recursive: true });

    // Copy all files from source to target
    const files = fs.readdirSync(ASSETS_SOURCE_DIR);
    files.forEach(file => {
        const sourcePath = path.join(ASSETS_SOURCE_DIR, file);
        const targetPath = path.join(ASSETS_TARGET_DIR, file);

        const stat = fs.statSync(sourcePath);
        if (stat.isDirectory()) {
            fs.cpSync(sourcePath, targetPath, { recursive: true });
        } else {
            fs.copyFileSync(sourcePath, targetPath);
        }
    });
} else {
    console.log('Skipping assets sync (--sync-assets not specified)');
}

console.log('Build and processing completed successfully!');
console.log(`Output files are located in '${OUTPUT_DIR}' with prefix '${CUSTOM_OUTPUT_NAME}'.`);
