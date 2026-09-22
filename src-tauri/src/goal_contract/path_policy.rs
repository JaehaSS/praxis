use std::path::{Component, Path};

pub(crate) fn valid_protected_pattern(value: &str) -> bool {
    if value.is_empty()
        || value.trim() != value
        || value.len() > 512
        || value.starts_with(['/', '\\'])
        || value.contains('\\')
        || value.as_bytes().get(1) == Some(&b':')
        || value.contains('?')
        || value.contains('[')
        || value.contains(']')
    {
        return false;
    }
    let supported_wildcard = !value.contains('*')
        || value == "*"
        || value == "**"
        || value
            .strip_suffix("/*")
            .is_some_and(|prefix| !prefix.contains('*'))
        || value
            .strip_suffix("/**")
            .is_some_and(|prefix| !prefix.contains('*'));
    supported_wildcard
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub fn protected_path_violations(patterns: &[String], changed: &[String]) -> Vec<String> {
    changed
        .iter()
        .filter(|path| {
            patterns
                .iter()
                .any(|pattern| matches_pattern(pattern, path))
        })
        .cloned()
        .collect()
}

fn matches_pattern(pattern: &str, path: &str) -> bool {
    if pattern == "**" || pattern == path {
        return true;
    }
    if pattern == "*" {
        return !path.contains('/');
    }
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    pattern.strip_suffix("/*").is_some_and(|prefix| {
        path.strip_prefix(&format!("{prefix}/"))
            .is_some_and(|rest| !rest.contains('/'))
    })
}
