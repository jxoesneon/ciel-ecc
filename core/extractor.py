import ast
import os
import json

class SymbolExtractor:
    def __init__(self, root_dir):
        self.root_dir = root_dir
        self.symbols = []
        self.relationships = []

    def extract_from_file(self, file_path):
        with open(file_path, "r") as f:
            try:
                tree = ast.parse(f.read())
            except SyntaxError:
                return

        rel_path = os.path.relpath(file_path, self.root_dir)
        
        for node in ast.walk(tree):
            if isinstance(node, ast.ClassDef):
                self.symbols.append({
                    "name": node.name,
                    "type": "class",
                    "file": rel_path,
                    "line": node.lineno
                })
                # Relationship: file contains class
                self.relationships.append((rel_path, "contains", node.name))
            
            elif isinstance(node, ast.FunctionDef):
                self.symbols.append({
                    "name": node.name,
                    "type": "function",
                    "file": rel_path,
                    "line": node.lineno
                })
                # Relationship: file contains function
                self.relationships.append((rel_path, "contains", node.name))
                
                # Check for calls within function
                for subnode in ast.walk(node):
                    if isinstance(subnode, ast.Call):
                        if isinstance(subnode.func, ast.Name):
                            self.relationships.append((node.name, "calls", subnode.func.id))
                        elif isinstance(subnode.func, ast.Attribute):
                            self.relationships.append((node.name, "calls", subnode.func.attr))

    def scan(self, max_files=100):
        blacklist = [".git", "node_modules", ".cache", ".npm", "__pycache__", ".ollama", ".agents", ".gemini"]
        file_count = 0
        for root, dirs, files in os.walk(self.root_dir):
            dirs[:] = [d for d in dirs if d not in blacklist]
            
            for file in files:
                if file.endswith(".py"):
                    file_path = os.path.join(root, file)
                    print(f"Scanning: {file_path}")
                    self.extract_from_file(file_path)
                    file_count += 1
                    if file_count >= max_files:
                        print("Reached file limit.")
                        return self.symbols, self.relationships
        return self.symbols, self.relationships

if __name__ == "__main__":
    import sys
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    ext = SymbolExtractor(root)
    syms, rels = ext.scan()
    print(f"Extracted {len(syms)} symbols and {len(rels)} relationships.")
    # Output first few for verification
    for rel in rels[:10]:
        print(f"  {rel[0]} --[{rel[1]}]--> {rel[2]}")
