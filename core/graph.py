import sqlite3
import json
import os

class CielGraph:
    def __init__(self, db_path="ciel_knowledge.db"):
        self.db_path = db_path
        self._init_db()

    def _init_db(self):
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                type TEXT DEFAULT 'unknown'
            )
        """)
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS triples (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                subject TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object TEXT NOT NULL,
                FOREIGN KEY (subject) REFERENCES entities(id),
                FOREIGN KEY (object) REFERENCES entities(id)
            )
        """)
        conn.commit()
        conn.close()

    def add_symbol(self, name, symbol_type):
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute("INSERT OR REPLACE INTO entities (id, name, type) VALUES (?, ?, ?)", 
                       (name.lower(), name, symbol_type))
        conn.commit()
        conn.close()

    def add_relationship(self, subject, predicate, obj):
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        # Ensure entities exist
        cursor.execute("INSERT OR IGNORE INTO entities (id, name) VALUES (?, ?)", (subject.lower(), subject))
        cursor.execute("INSERT OR IGNORE INTO entities (id, name) VALUES (?, ?)", (obj.lower(), obj))
        # Add triple
        cursor.execute("INSERT INTO triples (subject, predicate, object) VALUES (?, ?, ?)", 
                       (subject.lower(), predicate, obj.lower()))
        conn.commit()
        conn.close()

    def query(self, symbol_name):
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute("SELECT predicate, object FROM triples WHERE subject = ?", (symbol_name.lower(),))
        results = cursor.fetchall()
        conn.close()
        return results

if __name__ == "__main__":
    g = CielGraph()
    g.add_symbol("CielOrchestrator", "class")
    g.add_relationship("CielOrchestrator", "calls", "CielFSM")
    print(f"Query CielOrchestrator: {g.query('CielOrchestrator')}")
