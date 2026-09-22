const STANDARD_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];
const MAX_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const ALL_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultra"];
const AGY_EFFORTS: &[&str] = &["low", "medium", "high"];

/// 비어 있으면 CLI 설정 기본값을 상속하고, 명시값은 벤더별 닫힌 effort 집합만 허용한다.
pub fn reasoning_effort_override(
    agent: &str,
    effort: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(effort) = effort.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let allowed = match agent.trim() {
        "codex" => ALL_EFFORTS,
        // claude CLI(v2.1.218 실측)는 `--effort`를 모델과 무관하게 전역 검증(low~max, ultra 없음).
        "claude" => MAX_EFFORTS,
        "agy" | "gemini" | "antigravity" => AGY_EFFORTS,
        _ => {
            return Err(
                "reasoning effort는 Codex/Claude/Antigravity 작업에서만 설정할 수 있습니다"
                    .to_string(),
            )
        }
    };
    if !allowed.contains(&effort) {
        return Err(format!("지원하지 않는 reasoning effort입니다: {effort}"));
    }
    Ok(Some(effort.to_string()))
}

/// 알려진 모델은 capability matrix를 강제한다. 미선택·커스텀 모델은 공통 안전 범위만 허용한다.
/// claude는 모델별 matrix가 없어 `reasoning_effort_override`의 전역 검증을 그대로 따른다.
pub fn reasoning_effort_override_for_model(
    agent: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<Option<String>, String> {
    let effort = reasoning_effort_override(agent, effort)?;
    let Some(effort) = effort else {
        return Ok(None);
    };
    if agent.trim() != "codex" {
        return Ok(Some(effort));
    }
    let model = model.map(str::trim).unwrap_or_default();
    if supported_efforts(model).contains(&effort.as_str()) {
        return Ok(Some(effort));
    }
    let label = if model.is_empty() {
        "기본/미확인 모델"
    } else {
        model
    };
    Err(format!(
        "{label}에서 지원하지 않는 reasoning effort입니다: {effort}"
    ))
}

pub(crate) fn reasoning_effort_config_arg(effort: Option<&str>) -> Option<String> {
    reasoning_effort_override("codex", effort)
        .ok()
        .flatten()
        .map(|effort| format!("model_reasoning_effort=\"{effort}\""))
}

/// claude CLI용 `--effort <level>` 인자 — codex의 `reasoning_effort_config_arg`와 대칭.
pub(crate) fn claude_effort_args(effort: Option<&str>) -> Option<[String; 2]> {
    reasoning_effort_override("claude", effort)
        .ok()
        .flatten()
        .map(|effort| ["--effort".to_string(), effort])
}

/// Antigravity CLI용 `--effort <level>` 인자.
pub(crate) fn agy_effort_args(effort: Option<&str>) -> Option<[String; 2]> {
    reasoning_effort_override("agy", effort)
        .ok()
        .flatten()
        .map(|effort| ["--effort".to_string(), effort])
}

fn supported_efforts(model: &str) -> &'static [&'static str] {
    match model {
        "gpt-5.6-sol" | "gpt-5.6-terra" => ALL_EFFORTS,
        "gpt-5.6-luna" => MAX_EFFORTS,
        _ => STANDARD_EFFORTS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_allows_up_to_max_effort() {
        for effort in MAX_EFFORTS {
            assert_eq!(
                reasoning_effort_override("claude", Some(effort)),
                Ok(Some((*effort).to_string()))
            );
        }
    }

    #[test]
    fn claude_rejects_ultra() {
        assert!(reasoning_effort_override("claude", Some("ultra")).is_err());
    }

    #[test]
    fn unsupported_vendors_reject_reasoning_effort() {
        for agent in ["crush", "mybin --foo"] {
            assert!(reasoning_effort_override(agent, Some("high")).is_err());
        }
    }

    #[test]
    fn agy_aliases_allow_three_effort_levels_and_reject_higher_levels() {
        for agent in ["agy", "gemini", "antigravity"] {
            for effort in AGY_EFFORTS {
                assert_eq!(
                    reasoning_effort_override(agent, Some(effort)),
                    Ok(Some((*effort).to_string()))
                );
            }
            for effort in ["xhigh", "max", "ultra"] {
                assert!(reasoning_effort_override(agent, Some(effort)).is_err());
            }
        }
    }

    #[test]
    fn empty_or_blank_effort_inherits_regardless_of_agent() {
        assert_eq!(reasoning_effort_override("claude", None), Ok(None));
        assert_eq!(reasoning_effort_override("claude", Some("")), Ok(None));
        assert_eq!(reasoning_effort_override("claude", Some("  ")), Ok(None));
        assert_eq!(reasoning_effort_override("gemini", None), Ok(None));
    }

    #[test]
    fn claude_for_model_ignores_matrix_allows_custom_model_and_max() {
        assert_eq!(
            reasoning_effort_override_for_model("claude", Some("custom-model"), Some("max")),
            Ok(Some("max".to_string()))
        );
        assert_eq!(
            reasoning_effort_override_for_model("claude", None, Some("max")),
            Ok(Some("max".to_string()))
        );
    }

    #[test]
    fn claude_effort_args_builds_flag_pair() {
        assert_eq!(
            claude_effort_args(Some("high")),
            Some(["--effort".to_string(), "high".to_string()])
        );
        assert_eq!(claude_effort_args(None), None);
        assert_eq!(claude_effort_args(Some("")), None);
        assert_eq!(claude_effort_args(Some("ultra")), None);
    }

    #[test]
    fn agy_effort_args_builds_flag_pair_or_omits_inherited_effort() {
        assert_eq!(
            agy_effort_args(Some("high")),
            Some(["--effort".to_string(), "high".to_string()])
        );
        assert_eq!(agy_effort_args(None), None);
        assert_eq!(agy_effort_args(Some("  ")), None);
        assert_eq!(agy_effort_args(Some("xhigh")), None);
    }
}
