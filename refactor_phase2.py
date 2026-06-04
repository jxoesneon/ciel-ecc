import os
import re

agent_dir = 'temp_ecc/agents'
command_dir = 'temp_ecc/commands'

pairs = [
    ('code-reviewer.md', 'code-review.md'),
    ('cpp-reviewer.md', 'cpp-review.md'),
    ('fastapi-reviewer.md', 'fastapi-review.md'),
    ('flutter-reviewer.md', 'flutter-review.md'),
    ('go-reviewer.md', 'go-review.md'),
    ('kotlin-reviewer.md', 'kotlin-review.md'),
    ('python-reviewer.md', 'python-review.md'),
    ('react-reviewer.md', 'react-review.md'),
    ('rust-reviewer.md', 'rust-review.md'),
]

for agent_file, cmd_file in pairs:
    agent_path = os.path.join(agent_dir, agent_file)
    cmd_path = os.path.join(command_dir, cmd_file)
    
    agent_name = agent_file.replace('.md', '')
    
    # 1. Process Command File
    if os.path.exists(cmd_path):
        with open(cmd_path, 'r') as f:
            cmd_content = f.read()
            
        # Find "## Review Categories" or similar and remove it up to the next "## "
        # Patterns to look for: "## Review Categories", "## Review Priorities"
        new_cmd_content = []
        skip = False
        for line in cmd_content.splitlines():
            if line.startswith('## Review Categories') or line.startswith('## Review Priorities') or line.startswith('### CRITICAL'):
                skip = True
            elif skip and line.startswith('## ') and not line.startswith('## Review Categories') and not line.startswith('## Review Priorities'):
                skip = False
                new_cmd_content.append(line)
            elif not skip:
                new_cmd_content.append(line)
                
        # Add a note about delegation if it's not there
        # Let's insert it after "## What This Command Does" block
        cmd_text = '\n'.join(new_cmd_content)
        if f"Delegates expertise to the **{agent_name}** agent" not in cmd_text:
            cmd_text = cmd_text.replace("## What This Command Does", f"## Workflow\n\n*Note: This command defines the execution workflow. It delegates expertise and review priorities (CRITICAL/HIGH/MEDIUM) to the **{agent_name}** agent.*\n\n## What This Command Does")

        with open(cmd_path, 'w') as f:
            f.write(cmd_text + '\n')

    # 2. Process Agent File
    if os.path.exists(agent_path):
        with open(agent_path, 'r') as f:
            agent_content = f.read()
            
        new_agent_content = []
        skip = False
        in_when_invoked = False
        for line in agent_content.splitlines():
            if line.startswith('When invoked:'):
                in_when_invoked = True
                continue
            
            if in_when_invoked:
                # Stop skipping when we hit a blank line or a new header
                if line.strip() == '' or line.startswith('#'):
                    in_when_invoked = False
                else:
                    continue

            if line.startswith('## Diagnostic Commands') or line.startswith('## Automated Checks Run'):
                skip = True
            elif skip and line.startswith('## '):
                skip = False
                new_agent_content.append(line)
            elif not skip:
                new_agent_content.append(line)
                
        # We also might want to remove "## Approval Criteria" if that's in the command instead.
        # But let's stick to the immediate instruction: remove workflow from agents, expertise from commands.
        with open(agent_path, 'w') as f:
            f.write('\n'.join(new_agent_content) + '\n')

print("Refactored pairs!")
