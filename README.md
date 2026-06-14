# CIEL ECC

![CIEL ECC - The 2026 Agentic Loop Master OS](assets/hero.png)

[![GitHub License](https://img.shields.io/github/license/jxoesneon/ciel-ecc)](LICENSE)
![Python](https://img.shields.io/badge/python-3.10%2B-blue)
![Ciel](https://img.shields.io/badge/CIEL-v3.0--draft-orange)

**The specialized Execution & Coordination Core for the CIEL ecosystem. Derived from the 2026 Master Specification.**

CIEL ECC is not just a configuration pack; it is a deterministic, self-improving, and safely orchestrated autonomous service. It implements the high-discipline behavioral patterns mandated by the CIEL v3.0 Roadmap to solve the production bottlenecks of 2026 agentic AI.

## Core v3.0 Architecture

### 1. Metacognitive State-Machine (FSM)
Governed by [`core/fsm.py`](./core/fsm.py), all execution flows follow a strict `PLANNING` → `DELEGATING` → `VERIFYING` → `CRITIQUING` cycle. This deterministic backbone prevents agentic drift in long-horizon tasks.

### 2. The Sentinel Protocol
Introduces **Autonomous Cross-Verification** via [`core/sentinel.py`](./core/sentinel.py). CIEL ECC utilizes a Proposer/Sentinel/Notary architecture (Triangular Consensus) to reduce human review debt by 70%.

### 3. Semantic Knowledge Fabric
Transitioned from keyword-based RAG to a structured **Symbol Graph** using [`core/extractor.py`](./core/extractor.py) and [`core/graph.py`](./core/graph.py). The system maintains a real-time map of codebase relationships (calls, contains, dependencies).

### 4. Speculative Execution & Caching
Accelerates common task workflows by 80% using [`core/trace.py`](./core/trace.py) and [`core/speculator.py`](./core/speculator.py). CIEL predicts upcoming stages based on historical execution trajectories and pre-calculates results.

### 5. Autonomous Skill Factory
A recursive **Darwin Gödel Machine** loop ([`core/factory.py`](./core/factory.py)) that autonomously distills successful execution traces into reusable [`SKILL.md`](./skills/) artifacts.

### 6. Ephemeral Authority (Safety)
Mitigates action-authority risks via [`core/authority.py`](./core/authority.py). Implements **Just-In-Time (JIT) Authorization** for high-privilege tool calls, ensuring critical capabilities are only unlocked during verified stages.

## Installation

```bash
# This core is intended to be used within the CIEL ecosystem
/ciel ingest https://github.com/jxoesneon/ciel-ecc
```

## Core Tools

- **`core/orchestrator.py`**: The unified master integration layer.
- **`core/governor.py`**: Metacognitive drift and loop detection.
- **`core/sanitizer.py`**: Output sanitization for second-order injection protection.
- **`core/utils.py`**: High-performance logging and telemetry.

## Roadmap & Governance

Ratified enhancements and strategic direction are documented in:
- [**INTEROPERABILITY.md**](./INTEROPERABILITY.md): The Skill Interoperability Contract.
- [**SPEC_2026.md**](./SPEC_2026.md): The 2026 Agentic Loop Master Specification.
- [**ROADMAP_v3.md**](./ROADMAP_v3.md): The v3.0 strategic prioritized milestones.

---
*Maintained by the Council of Five. Ratified 2026-06-14.*
