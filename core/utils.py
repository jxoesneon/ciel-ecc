import datetime

class CielLogger:
    @staticmethod
    def log(message, level="INFO"):
        timestamp = datetime.datetime.now().isoformat()
        log_entry = f"[{timestamp}] [{level}] {message}\n"
        with open("ciel_core.log", "a") as f:
            f.write(log_entry)
        print(log_entry.strip())

if __name__ == "__main__":
    CielLogger.log("Logging utility initialized.")
