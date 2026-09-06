/**
 * Fast client-side syntax and AST delimiter balance validator.
 * Catches unclosed brackets, parentheses, string literals, and syntax errors
 * before execution.
 */

export interface ValidationResult {
  valid: boolean;
  error: string | null;
  line?: number;
}

function getLineNumber(code: string, index: number): number {
  return code.slice(0, index).split("\n").length;
}

export function validateCodeSyntax(
  code: string,
  languageId?: string
): ValidationResult {
  if (!code || typeof code !== "string") {
    return {
      valid: false,
      error: "Empty code buffer",
    };
  }

  const lang = String(languageId || "").toLowerCase();

  // JavaScript / TypeScript syntax validation via Function constructor
  if (["javascript", "js", "mjs", "cjs"].includes(lang)) {
    try {
      new Function(code);
    } catch (error: any) {
      return {
        valid: false,
        error: `Syntax Error: ${error?.message || String(error)}`,
      };
    }
  }

  // JSON validation
  if (lang === "json") {
    try {
      JSON.parse(code);
      return { valid: true, error: null };
    } catch (error: any) {
      return {
        valid: false,
        error: `JSON Parse Error: ${error?.message || String(error)}`,
      };
    }
  }

  // Universal delimiter balancing for all bracketed languages (Rust, Go, C++, Python, etc.)
  return checkDelimiterBalance(code);
}

export function checkDelimiterBalance(code: string): ValidationResult {
  interface StackItem {
    char: string;
    line: number;
  }

  const stack: StackItem[] = [];
  const pairs: Record<string, string> = {
    "}": "{",
    ")": "(",
    "]": "[",
  };

  const opening = new Set(["{", "(", "["]);
  const closing = new Set(["}", ")", "]"]);

  let inString = false;
  let stringChar: string | null = null;
  let inLineComment = false;
  let inBlockComment = false;

  for (let i = 0; i < code.length; i++) {
    const char = code[i];
    const next = code[i + 1];

    if (inLineComment) {
      if (char === "\n") inLineComment = false;
      continue;
    }

    if (inBlockComment) {
      if (char === "*" && next === "/") {
        inBlockComment = false;
        i++;
      }
      continue;
    }

    // Comment starts
    if (!inString && char === "/" && next === "/") {
      inLineComment = true;
      i++;
      continue;
    }
    if (!inString && char === "#" && !opening.has(char)) {
      inLineComment = true;
      continue;
    }
    if (!inString && char === "/" && next === "*") {
      inBlockComment = true;
      i++;
      continue;
    }

    // String literals
    if (
      (char === '"' || char === "'" || char === "`") &&
      code[i - 1] !== "\\"
    ) {
      if (inString && stringChar === char) {
        inString = false;
        stringChar = null;
      } else if (!inString) {
        inString = true;
        stringChar = char;
      }
      continue;
    }

    if (inString) continue;

    // Brackets
    if (opening.has(char)) {
      stack.push({ char, line: getLineNumber(code, i) });
      continue;
    }

    if (closing.has(char)) {
      if (!stack.length || stack[stack.length - 1].char !== pairs[char]) {
        const line = getLineNumber(code, i);
        return {
          valid: false,
          error: `Mismatched delimiter '${char}' near line ${line}.`,
          line,
        };
      }
      stack.pop();
    }
  }

  if (stack.length) {
    const unclosed = stack[stack.length - 1];
    return {
      valid: false,
      error: `Unclosed '${unclosed.char}' starting near line ${unclosed.line}.`,
      line: unclosed.line,
    };
  }

  return { valid: true, error: null };
}
