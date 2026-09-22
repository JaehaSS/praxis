pub const DEFAULT_ROLE: &str = "implementer";

pub const ROLES: &[&str] = &["planner", "researcher", DEFAULT_ROLE, "tester", "reviewer"];

pub fn normalize_role(role: &str) -> Result<&'static str, String> {
    let role = role.trim();
    ROLES
        .iter()
        .copied()
        .find(|candidate| *candidate == role)
        .ok_or_else(|| format!("role은 다음 중 하나여야 합니다: {}", ROLES.join(", ")))
}

pub fn normalize_role_or_default(role: &str) -> Result<&'static str, String> {
    if role.trim().is_empty() {
        return Ok(DEFAULT_ROLE);
    }
    normalize_role(role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_roles_and_defaults_omissions() {
        for role in ROLES {
            assert_eq!(normalize_role(role), Ok(*role));
        }
        assert_eq!(normalize_role_or_default(""), Ok(DEFAULT_ROLE));
        assert_eq!(normalize_role_or_default("  tester  "), Ok("tester"));
    }

    #[test]
    fn rejects_unknown_roles() {
        let error = normalize_role_or_default("manager").unwrap_err();
        assert!(error.contains("planner"));
        assert!(error.contains("reviewer"));
    }
}
