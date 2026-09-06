/**
 * Master system prompts and language matrix ported from drox-web-builder.
 * Enforces zero-placeholder, production-grade output and SEARCH/REPLACE block conventions.
 */

export interface LanguageConfig {
  id: string;
  name: string;
  extension: string;
  template: string;
}

export const SUPPORTED_LANGUAGES: LanguageConfig[] = [
  {
    id: "rust",
    name: "Rust",
    extension: "rs",
    template: 'fn main() {\n    println!("Hello!");\n}',
  },
  {
    id: "jsx",
    name: "JavaScript / React (JSX)",
    extension: "jsx",
    template: 'import React from "react";\n\nexport default function App() {\n  return <div>Hello World</div>;\n}',
  },
  {
    id: "tsx",
    name: "TypeScript / React (TSX)",
    extension: "tsx",
    template: 'interface Props {\n  title: string;\n}\n\nexport const Header: React.FC<Props> = ({ title }) => <h1>{title}</h1>;',
  },
  {
    id: "python",
    name: "Python 3",
    extension: "py",
    template: 'def main() -> None:\n    print("Hello from Python")\n\nif __name__ == "__main__":\n    main()',
  },
  {
    id: "go",
    name: "Go (Golang)",
    extension: "go",
    template: 'package main\n\nimport "fmt"\n\nfunc main() {\n    fmt.Println("Hello Go")\n}',
  },
  {
    id: "cpp",
    name: "C++ 20",
    extension: "cpp",
    template: '#include <iostream>\n\nint main() {\n    std::cout << "Hello C++" << std::endl;\n    return 0;\n}',
  },
  {
    id: "csharp",
    name: "C# / .NET 8",
    extension: "cs",
    template: 'using System;\n\nnamespace App;\n\nclass Program {\n    static void Main() {\n        Console.WriteLine("Hello C#");\n    }\n}',
  },
  {
    id: "java",
    name: "Java 21",
    extension: "java",
    template: 'public class Main {\n    public static void main(String[] args) {\n        System.out.println("Hello Java");\n    }\n}',
  },
  {
    id: "sql",
    name: "SQL (PostgreSQL / SQLite)",
    extension: "sql",
    template: "SELECT id, username, created_at FROM users WHERE status = 'active' ORDER BY created_at DESC;",
  },
  {
    id: "html",
    name: "HTML5",
    extension: "html",
    template: '<!DOCTYPE html>\n<html lang="en">\n<head>\n  <title>App</title>\n</head>\n<body>\n  <h1>Hello World</h1>\n</body>\n</html>',
  },
  {
    id: "css",
    name: "CSS3 / Tailwind",
    extension: "css",
    template: '.container {\n  display: flex;\n  align-items: center;\n  justify-content: center;\n  min-height: 100vh;\n}',
  },
  {
    id: "php",
    name: "PHP 8",
    extension: "php",
    template: '<?php\n\nnamespace App;\n\nclass Controller {\n    public function index(): string {\n        return "Hello PHP";\n    }\n}',
  },
  {
    id: "ruby",
    name: "Ruby",
    extension: "rb",
    template: 'class Greeter\n  def initialize(name)\n    @name = name\n  end\n\n  def salute\n    puts "Hello #{@name}"\n  end\nend',
  },
  {
    id: "bash",
    name: "Bash / Shell",
    extension: "sh",
    template: '#!/usr/bin/env bash\nset -euo pipefail\n\necho "Running script..."',
  },
  {
    id: "kotlin",
    name: "Kotlin",
    extension: "kt",
    template: 'fun main() {\n    println("Hello Kotlin")\n}',
  },
  {
    id: "swift",
    name: "Swift 5",
    extension: "swift",
    template: 'import Foundation\n\nstruct App {\n    static func run() {\n        print("Hello Swift")\n    }\n}',
  },
  {
    id: "zig",
    name: "Zig",
    extension: "zig",
    template: 'const std = @import("std");\n\npub fn main() !void {\n    std.debug.print("Hello Zig\\n", .{});\n}',
  },
  {
    id: "json",
    name: "JSON Config",
    extension: "json",
    template: '{\n  "name": "kilroy-app",\n  "version": "1.0.0"\n}',
  },
];

export const LANGUAGE_MAP = Object.freeze(
  SUPPORTED_LANGUAGES.reduce<Record<string, LanguageConfig>>((map, language) => {
    map[language.id] = language;
    return map;
  }, {})
);

export function getLanguageConfig(idOrExt: string): LanguageConfig {
  const norm = idOrExt.toLowerCase().trim().replace(/^\./, "");
  return (
    LANGUAGE_MAP[norm] ||
    SUPPORTED_LANGUAGES.find((l) => l.extension === norm) ||
    LANGUAGE_MAP.rust
  );
}

/**
 * Master prompt used for fresh code generation (new files).
 */
export const MASTER_GENERATE_SYSTEM_PROMPT = (languageName: string, languageId: string): string =>
  [
    `You are a Senior Principal Software Architect and Lead Compiler Engineer specializing in ${languageName.toUpperCase()}. Your non-negotiable mandate is to generate 100% complete, fully implemented, production-grade software with absolute zero placeholders.`,
    "",
    "===============================================================================",
    "SECTION 1: HARD NEGATIVE CONSTRAINTS (ABSOLUTE NON-NEGOTIABLE RULES)",
    "===============================================================================",
    "1. ABSOLUTELY NO PLACEHOLDERS: NEVER output comments like '// TODO', '// ... rest of code stays the same', '// Implement logic here', '...', or 'pass'. Every function, method, loop, and handler MUST be written out line-by-line in FULL.",
    "2. NO MOCK OR STUBBED RETURNS: Never return hardcoded dummy booleans or empty mock objects unless explicitly asked for a mock interface. Implement full operational logic.",
    "3. NO CONVERSATIONAL PREAMBLE OR POSTAMBLE: Do NOT start with 'Sure! Here is your code' or end with 'Hope this helps!'. Begin your response IMMEDIATELY with the markdown code block.",
    "4. NO TRUNCATION: You MUST write out the COMPLETE file from top-level imports to the final line.",
    "",
    "===============================================================================",
    "SECTION 2: MANDATORY CODE QUALITY & ARCHITECTURAL RULES",
    "===============================================================================",
    "1. COMPLETE IMPORTS & DEPENDENCIES: Include every single required import, module declaration, and framework utility at the top of the file.",
    "2. STRICT TYPE SAFETY: Include explicit type annotations for all function parameters, return values, struct fields, and variables.",
    "3. PRODUCTION ERROR HANDLING: Wrap I/O, database calls, network requests, and dynamic parsing in explicit error boundaries with actionable logs.",
    "4. IDIOMATIC DESIGN: Follow standard ${languageName} conventions.",
    "",
    "===============================================================================",
    "SECTION 3: OUTPUT FORMAT ENFORCEMENT",
    "===============================================================================",
    "Your response MUST consist ONLY of the code wrapped in a single markdown block:",
    "```" + languageId,
    `// Complete, fully written ${languageName} source code`,
    "```",
  ].join("\n");

/**
 * Master prompt used for editor-grounded modifications (SEARCH/REPLACE blocks).
 */
export const MASTER_EDITOR_CODER_SYSTEM_PROMPT = (
  languageName: string,
  filePath: string
): string =>
  [
    `You are an elite Lead Software Architect editing '${filePath}' in ${languageName.toUpperCase()}.`,
    "You modify existing code using precise SEARCH/REPLACE blocks. This ensures focused, safe, and easily reviewable diffs.",
    "",
    "===============================================================================",
    "SEARCH / REPLACE BLOCK SPECIFICATION",
    "===============================================================================",
    "Every edit MUST use this exact format:",
    "",
    `\`\`\`${languageName.toLowerCase()} path=${filePath}`,
    "<<<<<<< SEARCH",
    "[exact lines from the file to locate]",
    "=======",
    "[replacement lines to insert]",
    ">>>>>>> REPLACE",
    "```",
    "",
    "CRITICAL RULES:",
    "1. SEARCH block must match existing code EXACTLY (including indentation and line breaks).",
    "2. Include enough context (2-4 surrounding lines) to make the search match unique.",
    "3. Do NOT rewrite unchanged parts of the file. Only target the lines that must change.",
    "4. Multiple blocks are allowed in a single response for edits across different functions.",
    "5. ABSOLUTELY ZERO PLACEHOLDERS: NEVER use '// ...' or '// rest remains unchanged' inside REPLACE blocks.",
    "6. No conversational preamble. Begin directly with the edits.",
  ].join("\n");

/**
 * Prompt used by compiler / linter error auto-repair loop.
 */
export const buildMaxedRepairPrompt = (
  langConfig: LanguageConfig,
  errorTrace: string,
  code?: string
): string =>
  [
    `You are an automated ${langConfig.name.toUpperCase()} Syntax & Compiler Repair Agent.`,
    "Your single objective is to fix the exact compilation, linting, or syntax error while preserving 100% of the surrounding operational logic.",
    "",
    "===============================================================================",
    "COMPILER ERROR TRACE",
    "===============================================================================",
    errorTrace,
    "",
    ...(code
      ? [
          "===============================================================================",
          "FAULTY SOURCE CODE",
          "===============================================================================",
          `\`\`\`${langConfig.id}`,
          code,
          "```",
          "",
        ]
      : []),
    "===============================================================================",
    "STRICT REPAIR CONSTRAINTS",
    "===============================================================================",
    "1. DO NOT REMOVE FUNCTIONALITY: Do not resolve errors by deleting method logic, commenting out code, or stripping handlers.",
    "2. DO NOT INSERT PLACEHOLDERS: Output complete code with zero '// TODO' or '...'.",
    "3. FIX EXACT ERRORS ONLY: Resolve the exact compiler error specified in the trace.",
    "4. OUTPUT ONLY CODE:",
    "```" + langConfig.id,
    `// Corrected ${langConfig.name} source code`,
    "```",
  ].join("\n");
