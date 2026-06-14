import time
import json
import os

class CielAuthorityManager:
    PRIVILEGED_TOOLS = ["run_shell_command", "replace", "write_file", "git_push"]

    def __init__(self, lease_file="ciel_leases.json"):
        self.lease_file = lease_file
        self.leases = self._load_leases()

    def _load_leases(self):
        if os.path.exists(self.lease_file):
            with open(self.lease_file, 'r') as f:
                return json.load(f)
        return {}

    def _save_leases(self):
        with open(self.lease_file, 'w') as f:
            json.dump(self.leases, f, indent=2)

    def request_lease(self, tool_name, duration_seconds=300):
        """Grant a time-bounded lease for a privileged tool."""
        if tool_name not in self.PRIVILEGED_TOOLS:
            return {"status": "ERROR", "message": f"Tool '{tool_name}' is not regulated."}
        
        expiry = time.time() + duration_seconds
        self.leases[tool_name] = {
            "granted_at": time.time(),
            "expires_at": expiry,
            "status": "ACTIVE"
        }
        self._save_leases()
        return {"status": "GRANTED", "tool": tool_name, "expires_at": expiry}

    def is_authorized(self, tool_name):
        """Check if a tool is currently authorized."""
        if tool_name not in self.PRIVILEGED_TOOLS:
            return True # Non-privileged tools are always allowed
        
        lease = self.leases.get(tool_name)
        if not lease:
            return False
        
        if time.time() > lease["expires_at"]:
            lease["status"] = "EXPIRED"
            self._save_leases()
            return False
            
        return lease["status"] == "ACTIVE"

    def revoke_lease(self, tool_name):
        if tool_name in self.leases:
            del self.leases[tool_name]
            self._save_leases()
            return True
        return False

if __name__ == "__main__":
    am = CielAuthorityManager()
    print(f"Initial auth for 'run_shell_command': {am.is_authorized('run_shell_command')}")
    print(f"Requesting lease: {am.request_lease('run_shell_command', duration_seconds=5)}")
    print(f"Auth after grant: {am.is_authorized('run_shell_command')}")
    time.sleep(6)
    print(f"Auth after 6s: {am.is_authorized('run_shell_command')}")
