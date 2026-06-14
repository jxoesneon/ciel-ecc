import json
import os
import hashlib

class CielTraceStore:
    def __init__(self, trace_dir="ciel_traces"):
        self.trace_dir = trace_dir
        if not os.path.exists(self.trace_dir):
            os.makedirs(self.trace_dir)

    def _get_intent_hash(self, intent):
        return hashlib.sha256(intent.lower().strip().encode()).hexdigest()

    def record_trace(self, intent, plan, success=True):
        intent_hash = self._get_intent_hash(intent)
        trace_file = os.path.join(self.trace_dir, f"{intent_hash}.json")
        
        trace_data = {
            "intent": intent,
            "plan": plan,
            "success": success,
            "timestamp": hashlib.sha256(os.urandom(16)).hexdigest()[:8] # Simplified ID
        }
        
        # Append to existing or create new
        if os.path.exists(trace_file):
            with open(trace_file, 'r') as f:
                data = json.load(f)
                if isinstance(data, list):
                    data.append(trace_data)
                else:
                    data = [data, trace_data]
        else:
            data = [trace_data]
            
        with open(trace_file, 'w') as f:
            json.dump(data, f, indent=2)

    def get_similar_traces(self, intent):
        intent_hash = self._get_intent_hash(intent)
        trace_file = os.path.join(self.trace_dir, f"{intent_hash}.json")
        if os.path.exists(trace_file):
            with open(trace_file, 'r') as f:
                return json.load(f)
        return []

if __name__ == "__main__":
    ts = CielTraceStore()
    dummy_plan = {"stages": ["Stage 1", "Stage 2"]}
    ts.record_trace("test task", dummy_plan)
    print(f"Found {len(ts.get_similar_traces('test task'))} similar traces.")
