import type { Options } from '@wdio/types';
import path from 'path';

/**
 * WebdriverIO configuration for UNFUDGED Tauri desktop app E2E testing.
 *
 * This config uses tauri-driver to launch and control the UNFUDGED.app
 * installed via Homebrew. tauri-driver implements the W3C WebDriver protocol
 * and provides access to the Tauri app's WebView via standard selectors.
 *
 * Setup:
 * 1. Install tauri-driver: cargo install tauri-driver
 * 2. Install Node dependencies: npm install (in tests/e2e/)
 * 3. Run tests: npm run test:app
 *
 * Requirements:
 * - UNFUDGED.app installed at /Applications/UNFUDGED.app (via Homebrew cask)
 * - tauri-driver available in PATH
 * - macOS 10.15+ (from app's minimum supported version)
 */

export const config: Options.WebdriverIO = {
  runner: 'local',

  // Test specs to run
  specs: [
    './app-test.spec.ts'
  ],

  // WebdriverIO will start tauri-driver, which launches the Tauri app
  // and exposes a WebDriver server on localhost:4444
  capabilities: [{
    'tauri:options': {
      // Path to the UNFUDGED.app bundle (installed via Homebrew)
      application: '/Applications/UNFUDGED.app/Contents/MacOS/unfudged'
    }
  }],

  // WebDriver server port (tauri-driver uses 4444)
  port: 4444,

  // Framework for test structure
  framework: 'mocha',

  // Reporting
  reporters: ['spec'],

  // Mocha-specific options
  mochaOpts: {
    timeout: 60000,
    ui: 'bdd'
  },

  // Connection timeouts
  waitforTimeout: 10000,
  connectionRetryTimeout: 90000,
  connectionRetryCount: 3,

  // Clean up and prep
  onPrepare: async () => {
    console.log('Preparing UNFUDGED E2E tests...');
    console.log(`WebDriver server will start on http://localhost:4444`);
  },

  onComplete: async () => {
    console.log('UNFUDGED E2E tests complete');
  }
};
