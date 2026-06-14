from fsm import CielFSM
from governor import CielGovernor
from sanitizer import CielSanitizer
from utils import CielLogger
from extractor import SymbolExtractor
from graph import CielGraph
from speculator import CielSpeculator
from trace import CielTraceStore
from factory import CielSkillFactory
from authority import CielAuthorityManager
import sys
import os
import json

class CielOrchestrator:
    def __init__(self, root_dir="Ciel_repo/ciel-ecc"):
        self.fsm = CielFSM()
        self.gov = CielGovernor()
        self.san = CielSanitizer()
        self.graph = CielGraph()
        self.spec = CielSpeculator()
        self.trace_store = CielTraceStore()
        self.factory = CielSkillFactory()
        self.auth = CielAuthorityManager()
        self.root_dir = root_dir
        CielLogger.log("Orchestrator initialized", "INIT")
        self._build_knowledge_fabric()

    def _build_knowledge_fabric(self):
        CielLogger.log("Building Semantic Knowledge Fabric...")
        extractor = SymbolExtractor(self.root_dir)
        symbols, relationships = extractor.scan()
        
        for sym in symbols:
            self.graph.add_symbol(sym["name"], sym["type"])
        
        for rel in relationships:
            self.graph.add_relationship(rel[0], rel[1], rel[2])
            
        CielLogger.log(f"Fabric built: {len(symbols)} symbols, {len(relationships)} relationships.")

    def run_step(self, intent, tool_to_use=None):
        CielLogger.log(f"Processing intent: {intent}")
        
        # 0. Check for speculative plan
        speculation = self.spec.speculate(intent)
        if speculation:
            CielLogger.log(f"Speculative plan found (Confidence: {speculation['confidence']})", "SPEC")

        # 1. Check for drift
        drift_status = self.gov.check_drift(intent)
        if drift_status == "DRIFT_DETECTED":
            return "ERROR: Semantic drift detected. Realignment required."

        # 2. Check for authority if a privileged tool is requested
        if tool_to_use and not self.auth.is_authorized(tool_to_use):
            CielLogger.log(f"ACCESS DENIED: No active lease for tool '{tool_to_use}'", "SAFETY")
            return f"ERROR: Access denied for tool '{tool_to_use}'. Request authorization."

        # 3. Process based on current state
        state = self.fsm.get_state()
        print(f"[CIEL CORE] Current State: {state}")
        
        # Logic for each state would be implemented here
        return f"Processed intent in state {state}"

    def authorize_tool(self, tool_name, duration=300):
        CielLogger.log(f"Authorizing tool: {tool_name} for {duration}s", "SAFETY")
        return self.auth.request_lease(tool_name, duration)

    def finalize_task(self, intent, plan, success=True):
        CielLogger.log(f"Finalizing task: {intent}", "DONE")
        self.trace_store.record_trace(intent, plan, success)
        
        if success:
            CielLogger.log("Triggering Autonomous Skill Factory...", "EVOLVE")
            skill_path = self.factory.distill_from_trace({"intent": intent, "plan": plan})
            CielLogger.log(f"New skill distilled: {skill_path}", "EVOLVE")
            
        self.fsm.transition_to("DONE")

if __name__ == "__main__":
    orch = CielOrchestrator()
    if len(sys.argv) > 1:
        user_intent = sys.argv[1]
        tool_req = sys.argv[2] if len(sys.argv) > 2 else None
        
        if user_intent == "authorize" and tool_req:
            print(json.dumps(orch.authorize_tool(tool_req), indent=2))
        else:
            print(orch.run_step(user_intent, tool_req))
            
            # Simulation for the test case
            if "finalize" in user_intent.lower():
                sample_plan = {"summary": user_intent, "stages": ["Simulated Stage"]}
                orch.finalize_task(user_intent, sample_plan)
