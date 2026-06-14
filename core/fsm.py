import json
import os

class CielFSM:
    STATES = ["IDLE", "PLANNING", "DELEGATING", "VERIFYING", "CRITIQUING", "DONE"]

    def __init__(self, state_file="ciel_state.json"):
        self.state_file = state_file
        self.load_state()

    def load_state(self):
        if os.path.exists(self.state_file):
            with open(self.state_file, 'r') as f:
                self.data = json.load(f)
        else:
            self.data = {"state": "IDLE", "history": []}

    def save_state(self):
        with open(self.state_file, 'w') as f:
            json.dump(self.data, f, indent=2)

    def transition_to(self, new_state):
        if new_state in self.STATES:
            self.data["history"].append({"from": self.data["state"], "to": new_state})
            self.data["state"] = new_state
            self.save_state()
            return True
        return False

    def get_state(self):
        return self.data["state"]

if __name__ == "__main__":
    import sys
    fsm = CielFSM()
    if len(sys.argv) > 1:
        cmd = sys.argv[1]
        if cmd == "status":
            print(fsm.get_state())
        elif cmd == "transition" and len(sys.argv) > 2:
            if fsm.transition_to(sys.argv[2]):
                print(f"Transitioned to {sys.argv[2]}")
            else:
                print(f"Invalid state: {sys.argv[2]}")
                sys.exit(1)
