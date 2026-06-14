import json
import os
import subprocess

class CielSkillFactory:
    def __init__(self, output_dir="Ciel_repo/ciel-ecc/skills"):
        self.output_dir = output_dir
        if not os.path.exists(self.output_dir):
            os.makedirs(self.output_dir)

    def distill_from_trace(self, trace):
        intent = trace.get("intent", "unnamed-skill")
        plan = trace.get("plan", {})
        
        # 1. Normalize skill name
        skill_name = intent.lower().strip().replace(" ", "-")[:50]
        skill_path = os.path.join(self.output_dir, skill_name)
        
        if not os.path.exists(skill_path):
            os.makedirs(skill_path)
            
        # 2. Generate SKILL.md content
        # In v2, this would use a background LLM (e.g. Haiku) to synthesize instructions.
        # For the prototype, we use a structured template based on the success trace.
        
        description = f"Automated skill for: {intent}. Trigger when the user asks to {intent}."
        
        stages_md = "\n".join([f"- {stage}" for stage in plan.get("stages", [])])
        
        skill_md = f"""---
name: {skill_name}
description: {description}
---

# {intent.title()}

This skill was autonomously distilled by the CIEL Skill Factory based on a successful execution trace.

## Standardized Workflow

Following the success pattern from the original task, the following stages are recommended:

{stages_md}

## Automated Discipline
This skill inherits the 2026 Master Spec for Procedural Discipline (Plan -> Delegate -> Verify -> Critique).
"""
        
        with open(os.path.join(skill_path, "SKILL.md"), "w") as f:
            f.write(skill_md)
            
        return skill_path

if __name__ == "__main__":
    factory = CielSkillFactory()
    # Test with a dummy trace
    sample_trace = {
        "intent": "Setup high performance logging",
        "plan": {"stages": ["Install loguru", "Configure rotation", "Verify output"]}
    }
    path = factory.distill_from_trace(sample_trace)
    print(f"Skill distilled to: {path}")
