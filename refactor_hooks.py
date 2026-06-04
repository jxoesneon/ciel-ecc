import json
import os

with open('temp_ecc/hooks/hooks.json', 'r') as f:
    hooks_data = json.load(f)

registry = {}

for event, hook_list in hooks_data['hooks'].items():
    if event not in registry:
        registry[event] = {}
        
    for hook_entry in hook_list:
        matcher = hook_entry['matcher']
        if matcher not in registry[event]:
            registry[event][matcher] = []
            
        for cmd_entry in hook_entry['hooks']:
            cmd_str = cmd_entry['command']
            
            # Parse the command string
            # e.g., 'node -e "..." node scripts/hooks/run-with-flags.js pre:observe scripts/hooks/observe-runner.js standard,strict'
            
            parts = cmd_str.split('node scripts/hooks/')
            if len(parts) == 1:
                # Could be session-start-bootstrap.js without "node" before it?
                parts = cmd_str.split('\" node scripts/hooks/')
                if len(parts) == 1:
                    parts = cmd_str.split('\" scripts/hooks/')
                    
            if len(parts) > 1:
                target_part = parts[1].strip()
                # target_part is something like "run-with-flags.js pre:observe scripts/hooks/observe-runner.js standard,strict"
                # or "pre-bash-dispatcher.js"
                
                args = target_part.split(' ')
                base_script = args[0]
                
                if base_script == 'run-with-flags.js':
                    hook_id = args[1]
                    script_path = args[2]
                    profiles = args[3] if len(args) > 3 else '*'
                    registry[event][matcher].append({
                        'id': hook_id,
                        'script': script_path,
                        'profiles': profiles,
                        'async': cmd_entry.get('async', False),
                        'timeout': cmd_entry.get('timeout', 30000)
                    })
                else:
                    registry[event][matcher].append({
                        'id': hook_entry.get('id', 'legacy'),
                        'script': 'scripts/hooks/' + base_script,
                        'profiles': '*',
                        'async': cmd_entry.get('async', False),
                        'timeout': cmd_entry.get('timeout', 30000)
                    })

print("Registry parsed successfully!")

# Generate the dispatcher script
dispatcher_code = f"""#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const {{ spawnSync }} = require('child_process');
const {{ isHookEnabled }} = require('../lib/hook-flags');
const {{ buildPreToolUseAdditionalContext }} = require('./pretooluse-visible-output');

const HOOK_REGISTRY = {json.dumps(registry, indent=2)};

const MAX_STDIN = 1024 * 1024;

function readStdinRaw() {{
  return new Promise(resolve => {{
    let raw = '';
    let truncated = false;
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', chunk => {{
      if (raw.length < MAX_STDIN) {{
        const remaining = MAX_STDIN - raw.length;
        raw += chunk.substring(0, remaining);
        if (chunk.length > remaining) {{
          truncated = true;
        }}
      }} else {{
        truncated = true;
      }}
    }});
    process.stdin.on('end', () => resolve({{ raw, truncated }}));
    process.stdin.on('error', () => resolve({{ raw, truncated }}));
  }});
}}

function emitHookResult(raw, output) {{
  if (typeof output === 'string' || Buffer.isBuffer(output)) {{
    process.stdout.write(String(output));
    return 0;
  }}

  if (output && typeof output === 'object') {{
    if (output.stderr) process.stderr.write(output.stderr);
    
    if (Object.prototype.hasOwnProperty.call(output, 'additionalContext')) {{
      process.stdout.write(buildPreToolUseAdditionalContext(output.additionalContext));
    }} else if (Object.prototype.hasOwnProperty.call(output, 'stdout')) {{
      process.stdout.write(String(output.stdout ?? ''));
    }} else if (!Number.isInteger(output.exitCode) || output.exitCode === 0) {{
      process.stdout.write(raw);
    }}

    return Number.isInteger(output.exitCode) ? output.exitCode : 0;
  }}

  process.stdout.write(raw);
  return 0;
}}

function getPluginRoot() {{
  if (process.env.CLAUDE_PLUGIN_ROOT && process.env.CLAUDE_PLUGIN_ROOT.trim()) {{
    return process.env.CLAUDE_PLUGIN_ROOT;
  }}
  return path.resolve(__dirname, '..', '..');
}}

async function main() {{
  const [, , event, matcher] = process.argv;
  const {{ raw, truncated }} = await readStdinRaw();

  if (!event || !matcher) {{
    process.stdout.write(raw);
    process.exit(0);
  }}

  const hooksToRun = (HOOK_REGISTRY[event] && HOOK_REGISTRY[event][matcher]) || [];
  if (hooksToRun.length === 0) {{
    process.stdout.write(raw);
    process.exit(0);
  }}

  const pluginRoot = getPluginRoot();
  
  let currentRaw = raw;

  for (const hookDef of hooksToRun) {{
    if (!isHookEnabled(hookDef.id, {{ profiles: hookDef.profiles }})) {{
      continue;
    }}

    const scriptPath = path.resolve(pluginRoot, hookDef.script);
    if (!fs.existsSync(scriptPath)) {{
      process.stderr.write(`[Hook] Script not found for ${{hookDef.id}}: ${{scriptPath}}\\n`);
      continue;
    }}

    let hookModule;
    const src = fs.readFileSync(scriptPath, 'utf8');
    const hasRunExport = /\\bmodule\\.exports\\b/.test(src) && /\\brun\\b/.test(src);

    if (hasRunExport) {{
      try {{
        hookModule = require(scriptPath);
      }} catch (requireErr) {{
        process.stderr.write(`[Hook] require() failed for ${{hookDef.id}}: ${{requireErr.message}}\\n`);
      }}
    }}

    if (hookModule && typeof hookModule.run === 'function') {{
      try {{
        const output = hookModule.run(currentRaw, {{
          hookId: hookDef.id,
          pluginRoot,
          scriptPath,
          truncated,
          maxStdin: MAX_STDIN
        }});
        
        // If the hook returns modified output, we pass it to the next hook.
        // For simplicity, we assume hooks print to stdout/stderr or return an object.
        // Let's just run them and accumulate stderr/stdout.
        
        // This is a simplification for the central dispatcher.
        // In reality, some hooks are pre-tool and modify stdin. 
        // We'll emulate `run-with-flags.js` behavior closely.
        
        const exitCode = emitHookResult(currentRaw, output);
        if (exitCode !== 0) {{
           process.exit(exitCode);
        }}
      }} catch (runErr) {{
        process.stderr.write(`[Hook] run() error for ${{hookDef.id}}: ${{runErr.message}}\\n`);
      }}
    }} else {{
      // Legacy spawnSync
      const result = spawnSync(process.execPath, [scriptPath], {{
        input: currentRaw,
        encoding: 'utf8',
        env: {{
          ...process.env,
          CLAUDE_PLUGIN_ROOT: pluginRoot,
          ECC_PLUGIN_ROOT: pluginRoot,
          ECC_HOOK_ID: hookDef.id,
          ECC_HOOK_INPUT_TRUNCATED: truncated ? '1' : '0',
          ECC_HOOK_INPUT_MAX_BYTES: String(MAX_STDIN)
        }},
        cwd: process.cwd(),
        timeout: hookDef.timeout || 30000
      }});

      if (result.stderr) process.stderr.write(result.stderr);
      
      const stdout = typeof result.stdout === 'string' ? result.stdout : '';
      if (stdout) {{
        currentRaw = stdout; // pass to next hook
      }}

      if (result.error || result.signal || result.status !== 0) {{
        const failureDetail = result.error
          ? result.error.message
          : result.signal
            ? `terminated by signal ${{result.signal}}`
            : `exit status ${{result.status}}`;
        process.stderr.write(`[Hook] legacy hook execution failed for ${{hookDef.id}}: ${{failureDetail}}\\n`);
        if (result.status && result.status !== 0) process.exit(result.status);
      }}
    }}
  }}

  // Final output
  process.stdout.write(currentRaw);
  process.exit(0);
}}

main().catch(err => {{
  process.stderr.write(`[Hook] central-dispatcher error: ${{err.message}}\\n`);
  process.exit(0);
}});
"""

with open('temp_ecc/scripts/hooks/central-dispatcher.js', 'w') as f:
    f.write(dispatcher_code)

# 3. Create the new hooks.json
new_hooks = {"$schema": "https://json.schemastore.org/claude-code-settings.json", "hooks": {}}

INLINE_BOOTSTRAP = "node -e \"const p=require('path');const r=(()=>{var e=process.env.CLAUDE_PLUGIN_ROOT;if(e&&e.trim())return e.trim();var p=require('path'),f=require('fs'),h=require('os').homedir(),d=p.join(h,'.claude'),q=p.join('scripts','lib','utils.js');if(f.existsSync(p.join(d,q)))return d;for(var s of [['ecc'],['ecc@ecc'],['marketplaces','ecc'],['everything-claude-code'],['everything-claude-code@everything-claude-code'],['marketplaces','everything-claude-code']]){var l=p.join(d,'plugins',...s);if(f.existsSync(p.join(l,q)))return l}try{for(var g of ['ecc','everything-claude-code']){var b=p.join(d,'plugins','cache',g);for(var o of f.readdirSync(b,{withFileTypes:true})){if(!o.isDirectory())continue;for(var v of f.readdirSync(p.join(b,o.name),{withFileTypes:true})){if(!v.isDirectory())continue;var c=p.join(b,o.name,v.name);if(f.existsSync(p.join(c,q)))return c}}}}catch(x){}return d})();const s=p.join(r,'scripts/hooks/plugin-hook-bootstrap.js');process.env.CLAUDE_PLUGIN_ROOT=r;process.argv.splice(1,0,s);require(s)\""

for event, matchers in registry.items():
    new_hooks['hooks'][event] = []
    for matcher in matchers.keys():
        command_str = f'{INLINE_BOOTSTRAP} node scripts/hooks/central-dispatcher.js {event} "{matcher}"'
        
        is_async = any(h.get('async', False) for h in matchers[matcher])
        timeout = max([h.get('timeout', 30000) for h in matchers[matcher]] + [30000])

        hook_entry = {
            "matcher": matcher,
            "hooks": [
                {
                    "type": "command",
                    "command": command_str
                }
            ],
            "description": f"Centralized dispatcher for {event} ({matcher})",
            "id": f"ecc:central:{event.lower()}:{matcher.replace('|', '-')}"
        }
        if is_async:
            hook_entry['hooks'][0]['async'] = True
        hook_entry['hooks'][0]['timeout'] = timeout // 1000 if timeout > 30000 else timeout # Claude code uses seconds? Wait, Claude code uses numbers, the previous hooks had 10, 30, 300 which are probably seconds.
        
        # Keep original timeout if it was an int in original
        orig_timeouts = []
        for hook_data in hooks_data['hooks'].get(event, []):
            if hook_data['matcher'] == matcher:
                orig_timeouts.append(hook_data['hooks'][0].get('timeout', 30))
        max_timeout = max(orig_timeouts) if orig_timeouts else 30
        
        hook_entry['hooks'][0]['timeout'] = max_timeout
        new_hooks['hooks'][event].append(hook_entry)

with open('temp_ecc/hooks/hooks.json', 'w') as f:
    json.dump(new_hooks, f, indent=2)

print("Done! Central dispatcher generated and hooks.json refactored.")
