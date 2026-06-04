#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const { isHookEnabled } = require('../lib/hook-flags');
const { buildPreToolUseAdditionalContext } = require('./pretooluse-visible-output');

const HOOK_REGISTRY = {
  "PreToolUse": {
    "Bash": [
      {
        "id": "pre:bash:dispatcher",
        "script": "scripts/hooks/pre-bash-dispatcher.js",
        "profiles": "*",
        "async": false,
        "timeout": 30000
      }
    ],
    "Write": [
      {
        "id": "pre:write:doc-file-warning",
        "script": "scripts/hooks/doc-file-warning.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 30000
      }
    ],
    "Edit|Write": [
      {
        "id": "pre:edit-write:suggest-compact",
        "script": "scripts/hooks/suggest-compact.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 30000
      }
    ],
    "*": [
      {
        "id": "pre:observe",
        "script": "scripts/hooks/observe-runner.js",
        "profiles": "standard,strict",
        "async": true,
        "timeout": 10
      },
      {
        "id": "pre:mcp-health-check",
        "script": "scripts/hooks/mcp-health-check.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 30000
      }
    ],
    "Bash|Write|Edit|MultiEdit": [
      {
        "id": "pre:governance-capture",
        "script": "scripts/hooks/governance-capture.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 10
      }
    ],
    "Write|Edit|MultiEdit": [
      {
        "id": "pre:config-protection",
        "script": "scripts/hooks/config-protection.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 5
      }
    ],
    "Edit|Write|MultiEdit": [
      {
        "id": "pre:edit-write:gateguard-fact-force",
        "script": "scripts/hooks/gateguard-fact-force.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 5
      }
    ]
  },
  "PreCompact": {
    "*": [
      {
        "id": "pre:compact",
        "script": "scripts/hooks/pre-compact.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 30000
      }
    ]
  },
  "SessionStart": {
    "*": [
      {
        "id": "session:start",
        "script": "scripts/hooks/session-start-bootstrap.js",
        "profiles": "*",
        "async": false,
        "timeout": 30000
      }
    ]
  },
  "PostToolUse": {
    "Bash": [
      {
        "id": "post:bash:dispatcher",
        "script": "scripts/hooks/post-bash-dispatcher.js",
        "profiles": "*",
        "async": true,
        "timeout": 30
      }
    ],
    "Edit|Write|MultiEdit": [
      {
        "id": "post:quality-gate",
        "script": "scripts/hooks/quality-gate.js",
        "profiles": "standard,strict",
        "async": true,
        "timeout": 30
      },
      {
        "id": "post:edit:design-quality-check",
        "script": "scripts/hooks/design-quality-check.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 10
      },
      {
        "id": "post:edit:accumulate",
        "script": "scripts/hooks/post-edit-accumulator.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 30000
      }
    ],
    "Edit": [
      {
        "id": "post:edit:console-warn",
        "script": "scripts/hooks/post-edit-console-warn.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 30000
      }
    ],
    "Bash|Write|Edit|MultiEdit": [
      {
        "id": "post:governance-capture",
        "script": "scripts/hooks/governance-capture.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 10
      }
    ],
    "*": [
      {
        "id": "post:session-activity-tracker",
        "script": "scripts/hooks/session-activity-tracker.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 10
      },
      {
        "id": "post:observe",
        "script": "scripts/hooks/observe-runner.js",
        "profiles": "standard,strict",
        "async": true,
        "timeout": 10
      },
      {
        "id": "post:ecc-metrics-bridge",
        "script": "scripts/hooks/ecc-metrics-bridge.js",
        "profiles": "minimal,standard,strict",
        "async": false,
        "timeout": 10
      },
      {
        "id": "post:ecc-context-monitor",
        "script": "scripts/hooks/ecc-context-monitor.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 10
      }
    ]
  },
  "PostToolUseFailure": {
    "*": [
      {
        "id": "post:mcp-health-check",
        "script": "scripts/hooks/mcp-health-check.js",
        "profiles": "standard,strict",
        "async": false,
        "timeout": 30000
      }
    ]
  },
  "Stop": {
    "*": []
  },
  "SessionEnd": {
    "*": []
  }
};

const MAX_STDIN = 1024 * 1024;

function readStdinRaw() {
  return new Promise(resolve => {
    let raw = '';
    let truncated = false;
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', chunk => {
      if (raw.length < MAX_STDIN) {
        const remaining = MAX_STDIN - raw.length;
        raw += chunk.substring(0, remaining);
        if (chunk.length > remaining) {
          truncated = true;
        }
      } else {
        truncated = true;
      }
    });
    process.stdin.on('end', () => resolve({ raw, truncated }));
    process.stdin.on('error', () => resolve({ raw, truncated }));
  });
}

function emitHookResult(raw, output) {
  if (typeof output === 'string' || Buffer.isBuffer(output)) {
    process.stdout.write(String(output));
    return 0;
  }

  if (output && typeof output === 'object') {
    if (output.stderr) process.stderr.write(output.stderr);
    
    if (Object.prototype.hasOwnProperty.call(output, 'additionalContext')) {
      process.stdout.write(buildPreToolUseAdditionalContext(output.additionalContext));
    } else if (Object.prototype.hasOwnProperty.call(output, 'stdout')) {
      process.stdout.write(String(output.stdout ?? ''));
    } else if (!Number.isInteger(output.exitCode) || output.exitCode === 0) {
      process.stdout.write(raw);
    }

    return Number.isInteger(output.exitCode) ? output.exitCode : 0;
  }

  process.stdout.write(raw);
  return 0;
}

function getPluginRoot() {
  if (process.env.CLAUDE_PLUGIN_ROOT && process.env.CLAUDE_PLUGIN_ROOT.trim()) {
    return process.env.CLAUDE_PLUGIN_ROOT;
  }
  return path.resolve(__dirname, '..', '..');
}

async function main() {
  const [, , event, matcher] = process.argv;
  const { raw, truncated } = await readStdinRaw();

  if (!event || !matcher) {
    process.stdout.write(raw);
    process.exit(0);
  }

  const hooksToRun = (HOOK_REGISTRY[event] && HOOK_REGISTRY[event][matcher]) || [];
  if (hooksToRun.length === 0) {
    process.stdout.write(raw);
    process.exit(0);
  }

  const pluginRoot = getPluginRoot();
  
  let currentRaw = raw;

  for (const hookDef of hooksToRun) {
    if (!isHookEnabled(hookDef.id, { profiles: hookDef.profiles })) {
      continue;
    }

    const scriptPath = path.resolve(pluginRoot, hookDef.script);
    if (!fs.existsSync(scriptPath)) {
      process.stderr.write(`[Hook] Script not found for ${hookDef.id}: ${scriptPath}\n`);
      continue;
    }

    let hookModule;
    const src = fs.readFileSync(scriptPath, 'utf8');
    const hasRunExport = /\bmodule\.exports\b/.test(src) && /\brun\b/.test(src);

    if (hasRunExport) {
      try {
        hookModule = require(scriptPath);
      } catch (requireErr) {
        process.stderr.write(`[Hook] require() failed for ${hookDef.id}: ${requireErr.message}\n`);
      }
    }

    if (hookModule && typeof hookModule.run === 'function') {
      try {
        const output = hookModule.run(currentRaw, {
          hookId: hookDef.id,
          pluginRoot,
          scriptPath,
          truncated,
          maxStdin: MAX_STDIN
        });
        
        // If the hook returns modified output, we pass it to the next hook.
        // For simplicity, we assume hooks print to stdout/stderr or return an object.
        // Let's just run them and accumulate stderr/stdout.
        
        // This is a simplification for the central dispatcher.
        // In reality, some hooks are pre-tool and modify stdin. 
        // We'll emulate `run-with-flags.js` behavior closely.
        
        const exitCode = emitHookResult(currentRaw, output);
        if (exitCode !== 0) {
           process.exit(exitCode);
        }
      } catch (runErr) {
        process.stderr.write(`[Hook] run() error for ${hookDef.id}: ${runErr.message}\n`);
      }
    } else {
      // Legacy spawnSync
      const result = spawnSync(process.execPath, [scriptPath], {
        input: currentRaw,
        encoding: 'utf8',
        env: {
          ...process.env,
          CLAUDE_PLUGIN_ROOT: pluginRoot,
          ECC_PLUGIN_ROOT: pluginRoot,
          ECC_HOOK_ID: hookDef.id,
          ECC_HOOK_INPUT_TRUNCATED: truncated ? '1' : '0',
          ECC_HOOK_INPUT_MAX_BYTES: String(MAX_STDIN)
        },
        cwd: process.cwd(),
        timeout: hookDef.timeout || 30000
      });

      if (result.stderr) process.stderr.write(result.stderr);
      
      const stdout = typeof result.stdout === 'string' ? result.stdout : '';
      if (stdout) {
        currentRaw = stdout; // pass to next hook
      }

      if (result.error || result.signal || result.status !== 0) {
        const failureDetail = result.error
          ? result.error.message
          : result.signal
            ? `terminated by signal ${result.signal}`
            : `exit status ${result.status}`;
        process.stderr.write(`[Hook] legacy hook execution failed for ${hookDef.id}: ${failureDetail}\n`);
        if (result.status && result.status !== 0) process.exit(result.status);
      }
    }
  }

  // Final output
  process.stdout.write(currentRaw);
  process.exit(0);
}

main().catch(err => {
  process.stderr.write(`[Hook] central-dispatcher error: ${err.message}\n`);
  process.exit(0);
});
