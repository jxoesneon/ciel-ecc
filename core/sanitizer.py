import base64
import re

class CielSanitizer:
    DANGEROUS_PATTERNS = [
        r"ignore previous instructions",
        r"system prompt",
        r"you are now",
        r"new instructions:",
    ]

    @staticmethod
    def sanitize(text):
        # Basic pattern stripping
        for pattern in CielSanitizer.DANGEROUS_PATTERNS:
            text = re.sub(pattern, "[REDACTED_INJECTION_ATTEMPT]", text, flags=re.IGNORECASE)
        
        # Base64 encoding for safe re-entry
        return base64.b64encode(text.encode()).decode()

    @staticmethod
    def desanitize(encoded_text):
        return base64.b64decode(encoded_text.encode()).decode()

if __name__ == "__main__":
    import sys
    if len(sys.argv) > 1:
        content = " ".join(sys.argv[1:])
        print(CielSanitizer.sanitize(content))
