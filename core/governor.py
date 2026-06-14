import json
import os

class CielGovernor:
    def __init__(self, plan_file="ciel_plan.json"):
        self.plan_file = plan_file

    def load_plan(self):
        if os.path.exists(self.plan_file):
            with open(self.plan_file, 'r') as f:
                return json.load(f)
        return None

    def check_drift(self, current_intent):
        plan = self.load_plan()
        if not plan:
            return "NO_PLAN"
        
        # In a real implementation, this would call an LLM to compare intent with plan.
        # For this prototype, we'll implement a basic check.
        plan_summary = plan.get("summary", "")
        if not plan_summary:
            return "EMPTY_PLAN"
        
        # Placeholder for LLM-based drift detection
        return "SAFE"

if __name__ == "__main__":
    import sys
    gov = CielGovernor()
    if len(sys.argv) > 1:
        intent = " ".join(sys.argv[1:])
        print(gov.check_drift(intent))
