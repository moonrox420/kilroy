//! Native policy and safety checks for proposed actions.

use anyhow::{anyhow, Result};

pub fn evaluate_policy(content: &str) -> Result<()> {
    let lowered = content.to_ascii_lowercase();
    for (pattern, reason) in [
        ("exec(", "Dynamic exec is prohibited."),
        ("eval(", "Dynamic eval is prohibited."),
        (
            "os.system",
            "Unstructured os.system execution is prohibited.",
        ),
        (
            "subprocess.call",
            "Unstructured subprocess.call execution is prohibited.",
        ),
        ("except:", "Bare exception handlers hide failures."),
    ] {
        if lowered.contains(pattern) {
            return Err(anyhow!("proposal rejected by native policy: {reason}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_dangerous_patterns() {
        assert!(evaluate_policy("exec('malicious')").is_err());
        assert!(evaluate_policy("eval('malicious')").is_err());
        assert!(evaluate_policy("os.system('malicious')").is_err());
        assert!(evaluate_policy("except:\n    pass").is_err());
        assert!(evaluate_policy("let x = 42;").is_ok());
    }
}
