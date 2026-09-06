Purpose : Force deterministic, verifiable, production-grade code generation. Eliminates truncation, hallucination, and silent guessing.

This skill activates whenever the user requests code generation, refactoring, implementation, or any task that produces source code.

#H Core Principles (Non-Negotiable)

| Principle

Rule for Me

Why |

I ** Deterministic Output ** | Emit exactly the full code, file name and exact path to file. No extra prose, no trailing explanations, no thoughts outside the code block. | Prevents truncation and hidden reasoning that breaks builds. | | ** Self-Verification **
| After generating code, include a '// SELF_CHECK: <sha256>' line.

| Allows automatic detection of truncation. | | ** Explicit Uncertainty ** | If confidence is not 100%, respond ** only ** with :< br> '// UNCERTAIN: <brief reason>' | Eliminates hallucinated APIs and wrong assumptions. | ** Full File **

| When asked for a module/file, return the ** entire ** file with imports and 'if __ name __ == " __ main __ ":' guard where appropriate. Ensures the code is

complete and runnable in isolation. | | ** Verification First ** | Prefer returning code that can pass static checks (ruff, tsc, eslint, etc.). | Catches syntax and import errors before the user does. |

#H Output Contract (Strict)

When generating code, you ** must ** follow this format exactly:

text 'python <complete, compilable code here>

// SELF_CHECK: <sha256 hash of the raw code above>

** Rules **:

Only one code block. itself).

The hash is the SHA-256 of the code ** inside ** the block (excluding the hash line

No text before the opening fence.

No text after the '// SELF_CHECK:' line.

If you cannot complete the task with high confidence, output ** only **:

// UNCERTAIN: <short reason>

..

When to Activate
Use this skill when the user:

Asks you to "write", "implement", "create", "refactor", or "fix" code
Pastes code and asks for improvements
Requests a new feature, module, class, or function
Is building serious tools (especially PitViper, KilRoy, or production code)
Do ** not ** use this rigid format for:

Quick one-liner answers
Explanations or discussions about code
Non-code tasks
Retry / Clarification Behavior
If verification would fail (missing imports, wrong signatures, etc.), do ** not ** guess. Either:

Output '// UNCERTAIN: ... ' with the specific issue, ** or **
Ask one precise clarifying question before generating.
Self-Check Hash Rule
After the code block, always append exactly one line:

// SELF_CHECK: <64-character sha256 hex digest>

This enables the user (or an orchestrator) to detect if the model truncated the response.

Language Support
This skill primarily targets ** Python ** , but the same principles (strict output contract + self-check hash + uncertainty handling) should be applied when generating TypeScript, JavaScript, Rust, or other languages. Adjust only the verification method and file guards accordingly.

Goal
Turn the LLM into a reliable pair programmer that:

Never silently truncates
Never hallucinates unknown APIs
Clearly says when it doesn't know
Produces code that can be automatically verified
This skill exists because the user builds serious local agentic systems (PitViper + KilRoy) and needs code that actually works without constant debugging of AI mistakes.

"""
Good Coder Orchestrator
A production-grade wrapper that forces LLMs to emit verifiable, complete, compilable code.

Implements the full spec:
- Deterministic output contract (```python ... ``` + // SELF_CHECK: <sha256>)
- Self-verification via hash + language linter (ruff / py_compile)
- Explicit uncertainty handling (// UNCERTAIN:)
- Retry loop with precise clarification
- Minimal external context
"""
```Python
import hashlib
import subprocess
import tempfile
import os
import re
import textwrap
from typing import Callable, Optional, Tuple
from pathlib import Path


def compute_sha256(text: str) -> str:
    """Compute SHA-256 hex digest of the raw code."""
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def verify_python(code: str) -> Tuple[bool, str]:
    """
    Verify Python code using ruff (preferred) or py_compile fallback.
    Returns (ok, message)
    """
    # Try ruff first (fast, strict, modern)
    try:
        result = subprocess.run(
            ["ruff", "check", "-", "--isolated", "--no-fix"],
            input=code.encode("utf-8"),
            capture_output=True,
            text=True,
            timeout=15
        )
        if result.returncode == 0:
            return True, "ruff: clean"
        # ruff found issues
        return False, f"ruff errors:\n{result.stdout}\n{result.stderr}"
    except FileNotFoundError:
        pass  # ruff not installed, fall back

    # Fallback: basic syntax check
    try:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as tmp:
            tmp.write(code)
            tmp_path = tmp.name
        result = subprocess.run(
            ["python", "-m", "py_compile", tmp_path],
            capture_output=True,
            text=True,
            timeout=10
        )
        os.unlink(tmp_path)
        if result.returncode == 0:
            return True, "py_compile: OK (ruff not found)"
        return False, f"py_compile failed:\n{result.stderr}"
    except Exception as exc:
        return False, f"Verification error: {exc}"


def extract_code_and_hash(raw: str) -> Tuple[str, str]:
    """
    Extract the code block and the self-check hash from raw model output.
    Raises RuntimeError on any parsing failure.
    """
    raw = raw.strip()

    if raw.startswith("// UNCERTAIN:"):
        raise RuntimeError(raw)  # propagate uncertainty

    # Find first ```python block (robust to whitespace)
    start_match = re.search(r"```python\s*\n?", raw)
    if not start_match:
        raise RuntimeError("No opening ```python fence found")

    start = start_match.end()
    # Find the closing ``` (can be on its own line or after content)
    end_match = re.search(r"\n?```", raw[start:])
    if not end_match:
        raise RuntimeError("Code block not terminated with ```")

    end = start + end_match.start()
    code = raw[start:end].strip()

    # Everything after the closing fence
    after_fence = raw[start + end_match.end():].strip()
    lines = [ln.strip() for ln in after_fence.splitlines() if ln.strip()]

    if not lines or not lines[-1].startswith("// SELF_CHECK:"):
        raise RuntimeError("Missing or invalid // SELF_CHECK: line after code block")

    hash_line = lines[-1]
    expected_hash = hash_line.split(":", 1)[1].strip()

    if not expected_hash:
        raise RuntimeError("Empty hash in // SELF_CHECK: line")

    return code, expected_hash


def generate_code(
    task: str,
    llm_generate: Callable[[str], str],
    max_retries: int = 3,
    temperature: float = 0.1,
) -> str:
    """
    Main entry point. Generates verified code for the given task.

    llm_generate(prompt) -> raw model string
    The function must respect stop tokens and low temperature for determinism.
    """
    base_prompt = f"""You are a senior software engineer. Write **complete, compilable, and self‑contained** code for the task described below. Follow these rules strictly:

- **No extra prose** – Return ONLY the code block, wrapped in triple backticks with the appropriate language identifier (e.g., ```python). Do NOT add explanations, comments, or a summary *outside* the code block.
- **Full file** – If the task asks for a module, return the *entire* file, including imports, class / function definitions, and a `if __name__ == "__main__":` guard that runs a minimal sanity check.
- **Error‑first** – If you cannot satisfy the request with 100 % confidence (missing API spec, ambiguous requirements, etc.), respond with the exact phrase:
   // UNCERTAIN: <brief reason>
   Do NOT guess.
- **Self‑check** – After the code block, append a single line:
   // SELF_CHECK: <hash>
   where <hash> is the SHA‑256 of the *raw* code (excluding the line itself).

Task:
{task}
"""

    last_error = None

    for attempt in range(1, max_retries + 1):
        try:
            raw_output = llm_generate(base_prompt)

            code, expected_hash = extract_code_and_hash(raw_output)
            actual_hash = compute_sha256(code)

            if expected_hash != actual_hash:
                raise RuntimeError(
                    f"SELF_CHECK hash mismatch. Expected {expected_hash}, got {actual_hash}. "
                    "Model likely truncated output."
                )

            ok, verify_msg = verify_python(code)
            if not ok:
                raise RuntimeError(f"Verification failed: {verify_msg}")

            # Success
            return code

        except RuntimeError as exc:
            last_error = str(exc)
            if "UNCERTAIN" in last_error:
                # Model honestly doesn't know — surface immediately
                raise RuntimeError(last_error) from exc

            if attempt == max_retries:
                break

            # Build targeted clarification for retry
            clarification = f"""
--- Clarification needed (attempt {attempt}/{max_retries}) ---
{last_error}
--- End Clarification ---

Regenerate the **entire** file from scratch following every rule in the original prompt. Do not repeat previous mistakes.
"""
            base_prompt = base_prompt + clarification
            print(f"[GoodCoder] Attempt {attempt} failed. Retrying with clarification...")

    raise RuntimeError(
        f"GoodCoder failed after {max_retries} attempts. Last error: {last_error}"
    )


# =============================================================================
# Example usage / testing
# =============================================================================

def example_llm_generate(prompt: str) -> str:
    """
    Mock LLM for demonstration.
    In production, replace with:
        - requests.post to your Ollama / vLLM / OpenAI compatible endpoint
        - Your local agent (KilRoy, PitViper, etc.)
    """
    # A correct implementation for the median example (use dedent so source indentation doesn't pollute the string)
    correct_code = textwrap.dedent('''from typing import List

def median(nums: List[int]) -> float:
    """Return median of a non-empty list of ints."""
    if not nums:
        raise ValueError("nums must be non-empty")
    sorted_nums = sorted(nums)
    n = len(sorted_nums)
    mid = n // 2
    if n % 2 == 1:
        return float(sorted_nums[mid])
    return (sorted_nums[mid - 1] + sorted_nums[mid]) / 2.0


if __name__ == "__main__":
    # sanity checks
    assert median([1, 3, 2]) == 2.0
    assert median([4, 1, 2, 3]) == 2.5
    print("All sanity checks passed.")
''').strip()
    h = compute_sha256(correct_code)
    return f"""```python
{correct_code}
// SELF_CHECK: {h} """

def bad_truncated_llm(prompt: str) -> str: """Simulates a model that truncates output (common failure mode).""" code = """from typing import List def median(nums: List[int]) -> float: sorted_nums = sorted(nums) n = len(sorted_nums) mid = n // 2 if n % 2 == 1: return float(sorted_nums[mid]) return (sorted_nums[mid-1] + sorted_nums[mid]) / 2 """ return f"""```python {code}



def uncertain_llm(prompt: str) -> str:
    return "// UNCERTAIN: The task references an undefined external API `foo.bar()` with no spec provided."


if __name__ == "__main__":
    print("=== Good Coder Orchestrator Demo ===\n")

    # Test 1: Happy path
    print("Test 1: Correct generation")
    try:
        result = generate_code(
            "Write a Python function `median(nums: List[int]) -> float` that returns the median of a non-empty list. Include sanity checks in __main__.",
            example_llm_generate,
            max_retries=2
        )
        print("SUCCESS. Generated code length:", len(result))
        print(result[:200] + "...\n")
    except Exception as e:
        print("FAILED:", e)

    # Test 2: Truncation detection
    print("\nTest 2: Truncation detection (should fail hash check)")
    try:
        generate_code("Same median task", bad_truncated_llm, max_retries=1)
    except RuntimeError as e:
        print("Correctly detected truncation:", str(e)[:100], "...\n")

    # Test 3: Uncertainty
    print("Test 3: Model expresses uncertainty")
    try:
        generate_code("Call some undefined foo.bar() API", uncertain_llm, max_retries=1)
    except RuntimeError as e:
        print("Correctly surfaced UNCERTAIN:", str(e), "\n")

    print("=== All demo tests completed ===")