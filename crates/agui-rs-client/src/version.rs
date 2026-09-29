//! The protocol version this client speaks, and the arbitration around it.
//!
//! Mirrors `sdks/typescript/packages/core/src/generated/version.ts`
//! (`export const PROTOCOL_VERSION = "1.0";`, *"Never typed by a human"*) and
//! the producer-declaration rules in `agent/agent.ts:63-102`.

/// The protocol version this client declares and judges a producer against.
pub const PROTOCOL_VERSION: &str = "1.0";

/// How a producer's `RUN_STARTED.protocolVersion` reads against ours.
///
/// The grammar check runs BEFORE any comparison, exactly as upstream does:
/// `compare-versions` would read `"1"`, `"1.0.0"` or `"1.x"` as equal to
/// `"1.0"`, and the spec says a value outside the grammar is handled like a
/// newer one, not silently accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredVerdict {
    /// Outside `/^\d+\.\d+$/` — this client cannot read it at all.
    Uninterpretable,
    /// Newer than us: material we may be stripping deserves a voice.
    Newer,
    /// Older or equal — the quiet downgrade signal, silent by rule.
    NotNewer,
}

pub fn compare_declared_protocol(declared: &str, spoken: &str) -> DeclaredVerdict {
    if !is_published_grammar(declared) {
        return DeclaredVerdict::Uninterpretable;
    }
    if compare_versions(declared, spoken) > 0 {
        DeclaredVerdict::Newer
    } else {
        DeclaredVerdict::NotNewer
    }
}

/// Reads the producer's `RUN_STARTED` declaration. `None`, or our own version,
/// is silent; everything else is one of the two warnings.
///
/// `agent/agent.ts:84-102`.
pub fn warn_on_producer_declaration(declared: Option<&str>) {
    let Some(declared) = declared else {
        return;
    };
    if declared == PROTOCOL_VERSION {
        return;
    }
    match compare_declared_protocol(declared, PROTOCOL_VERSION) {
        DeclaredVerdict::Uninterpretable => eprintln!(
            "[ag-ui] The producer declared protocol version '{declared}', which this client cannot interpret."
        ),
        DeclaredVerdict::Newer => eprintln!(
            "[ag-ui] The producer speaks protocol {declared}; this client speaks {PROTOCOL_VERSION}. Unrecognised material will be stripped with warnings."
        ),
        DeclaredVerdict::NotNewer => {}
    }
}

/// `compare-versions` semantics, restricted to what this client needs: split
/// on `.`, compare a numeric segment numerically and anything else
/// lexicographically, and let a longer version win only when it says something
/// the shorter one does not.
pub fn compare_versions(left: &str, right: &str) -> i32 {
    let mut left = left.split('.');
    let mut right = right.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return 0,
            (None, Some(_)) => return -1,
            (Some(_), None) => return 1,
            (Some(a), Some(b)) => {
                let ordering = match (a.parse::<u64>(), b.parse::<u64>()) {
                    (Ok(a), Ok(b)) => a.cmp(&b),
                    _ => a.cmp(b),
                };
                match ordering {
                    std::cmp::Ordering::Less => return -1,
                    std::cmp::Ordering::Greater => return 1,
                    std::cmp::Ordering::Equal => {}
                }
            }
        }
    }
}

fn is_published_grammar(value: &str) -> bool {
    let mut parts = value.split('.');
    let two_numeric = parts.next().is_some_and(is_digits) && parts.next().is_some_and(is_digits);
    two_numeric && parts.next().is_none()
}

fn is_digits(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_is_two_numeric_components() {
        assert!(is_published_grammar("1.0"));
        assert!(is_published_grammar("12.34"));
        assert!(!is_published_grammar("1"));
        assert!(!is_published_grammar("1.0.0"));
        assert!(!is_published_grammar("1.x"));
        assert!(!is_published_grammar("v1.0"));
        assert!(!is_published_grammar(""));
    }

    #[test]
    fn three_way_verdicts() {
        assert_eq!(
            compare_declared_protocol("2.0", "1.0"),
            DeclaredVerdict::Newer
        );
        assert_eq!(
            compare_declared_protocol("0.9", "1.0"),
            DeclaredVerdict::NotNewer
        );
        assert_eq!(
            compare_declared_protocol("1.0", "1.0"),
            DeclaredVerdict::NotNewer
        );
        assert_eq!(
            compare_declared_protocol("1.x", "1.0"),
            DeclaredVerdict::Uninterpretable
        );
        // Compare-versions would call these equal; the grammar gate is what
        // stops that reaching a silent pass.
        assert_eq!(
            compare_declared_protocol("1", "1.0"),
            DeclaredVerdict::Uninterpretable
        );
        assert_eq!(
            compare_declared_protocol("1.0.0", "1.0"),
            DeclaredVerdict::Uninterpretable
        );
    }

    #[test]
    fn version_comparison() {
        assert_eq!(compare_versions("1.0", "1.0"), 0);
        assert_eq!(compare_versions("1.0.0", "1.0.0"), 0);
        assert_eq!(compare_versions("0.1.3", "0.1.3"), 0);
        assert!(compare_versions("0.9", "1.0") < 0);
        assert!(compare_versions("1.1", "1.0") > 0);
        assert!(compare_versions("2.0", "1.9.9") > 0);
    }
}
