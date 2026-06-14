# CIEL: Skill Interoperability Contract (v1.0.0)

This contract defines the orchestration standard for the 400+ skills in the Ciel ecosystem, ensuring seamless cooperation between **Procedural Discipline (Fable)**, **Governance (ECC)**, and **Domain Specialists**.

## 1. The Behavioral Skeleton (fable-mode)
- **Mandatory for High-Entropy Tasks**: Any task spanning >3 turns, >3 files, or involving cross-domain coordination MUST follow the `fable-mode` lifecycle (Plan -> Delegate -> Verify -> Critique).
- **Efficiency Override (Lazy Integration)**: For one-shot or established tool-use patterns (e.g., `gws` list, quick `git` check), skip the full Fable loop to maintain OODA velocity.

## 2. Governance & Specialist Routing (ECC Guilds)
- **Guild-Based Activation**: Use ECC Guild logic (Research, Systems, Strategy) to select specialists.
- **Recursive Audit**: Before executing a Fable stage, run a "Skill Audit" using `find-skills` to prevent reinventing established specialist capabilities.

## 3. The Truth Gate (Verification)
- **Empirical Artifacts**: Fable's "Stage 3" MUST utilize the `verification-loop` (tests, lints, or data assertions). "Vibes-based" verification is prohibited.
- **Safety Integrity**: Every verification check must include a "Freeze Mode" audit to ensure no unauthorized files were modified.

## 4. Resource Stewardship
- **Context Management**: Mandatory `strategic-compact` at the completion of each major Fable stage.
- **Safety Injection**: All sub-agent delegations MUST include the parent's `safety-guard` posture and directory constraints.

## 5. Evolution Loop
- **Instinct Capture**: Use `continuous-learning-v2` to observe and capture atomic success patterns.
- **Promotion**: High-confidence instincts are periodically promoted to formal Skills and added to the Guild catalogs.

---
*Ratified by the Council of Five on 2026-06-14.*
