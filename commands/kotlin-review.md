---
description: Comprehensive Kotlin code review for idiomatic patterns, null safety, coroutine safety, and security. Invokes the kotlin-reviewer agent.
---

# Kotlin Code Review

This command invokes the **kotlin-reviewer** agent for comprehensive Kotlin-specific code review.

## Workflow

*Note: This command defines the execution workflow. It delegates expertise and review priorities (CRITICAL/HIGH/MEDIUM) to the **kotlin-reviewer** agent.*

## What This Command Does

1. **Identify Kotlin Changes**: Find modified `.kt` and `.kts` files via `git diff`
2. **Run Build & Static Analysis**: Execute `./gradlew build`, `detekt`, `ktlintCheck`
3. **Security Scan**: Check for SQL injection, command injection, hardcoded secrets
4. **Null Safety Review**: Analyze `!!` usage, platform type handling, unsafe casts
5. **Coroutine Review**: Check structured concurrency, dispatcher usage, cancellation
6. **Generate Report**: Categorize issues by severity

## When to Use

Use `/kotlin-review` when:
- After writing or modifying Kotlin code
- Before committing Kotlin changes
- Reviewing pull requests with Kotlin code
- Onboarding to a new Kotlin codebase
- Learning idiomatic Kotlin patterns

## Automated Checks Run

```bash
# Build check
./gradlew build

# Static analysis
./gradlew detekt

# Formatting check
./gradlew ktlintCheck

# Tests
./gradlew test
```

## Example Usage

````text
User: /kotlin-review

Agent:
# Kotlin Code Review Report

## Files Reviewed
- src/main/kotlin/com/example/service/UserService.kt (modified)
- src/main/kotlin/com/example/routes/UserRoutes.kt (modified)

## Static Analysis Results
✓ Build: Successful
✓ detekt: No issues
WARNING: ktlint: 2 formatting warnings

## Issues Found

[CRITICAL] Force-Unwrap Null Safety
File: src/main/kotlin/com/example/service/UserService.kt:28
Issue: Using !! on nullable repository result
```kotlin
val user = repository.findById(id)!!  // NPE risk
```
Fix: Use safe call with error handling
```kotlin
val user = repository.findById(id)
    ?: throw UserNotFoundException("User $id not found")
```

[HIGH] GlobalScope Usage
File: src/main/kotlin/com/example/routes/UserRoutes.kt:45
Issue: Using GlobalScope breaks structured concurrency
```kotlin
GlobalScope.launch {
    notificationService.sendWelcome(user)
}
```
Fix: Use the call's coroutine scope
```kotlin
launch {
    notificationService.sendWelcome(user)
}
```

## Summary
- CRITICAL: 1
- HIGH: 1
- MEDIUM: 0

Recommendation: FAIL: Block merge until CRITICAL issue is fixed
````

## Approval Criteria

| Status | Condition |
|--------|-----------|
| PASS: Approve | No CRITICAL or HIGH issues |
| WARNING: Warning | Only MEDIUM issues (merge with caution) |
| FAIL: Block | CRITICAL or HIGH issues found |

## Integration with Other Commands

- Use `/kotlin-test` first to ensure tests pass
- Use `/kotlin-build` if build errors occur
- Use `/kotlin-review` before committing
- Use `/code-review` for non-Kotlin-specific concerns

## Related

- Agent: `agents/kotlin-reviewer.md`
- Skills: `skills/kotlin-patterns/`, `skills/kotlin-testing/`
