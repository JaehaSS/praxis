//! fd 한도 보강 — macOS GUI(.app)는 Finder/LaunchServices 실행 시 launchd의 `maxfiles`
//! **soft limit 256**을 상속한다 (로그인 셸의 `ulimit`과 무관하다). Praxis 본체가 WKWebView
//! 리소스만으로 90여 개를 쓰는 데다 세션마다 PTY master·파이프가 붙으므로, 동시 세션이
//! 겹치면 `claude` spawn이 `EMFILE`("Too many open files", os error 24)로 실패한다.
//!
//! 시작 시 soft를 hard까지 올려 두면 **spawn되는 자식(claude, 그 아래 MCP 서버)도 이를
//! 상속**하므로 두 층이 한 번에 풀린다. `envpath::augment_path()`와 같은 성격의 보강이라
//! 같은 자리에서 수행한다.
//!
//! 사다리 계산은 순수(테스트 대상), get/setrlimit 호출은 cfg(unix).

/// 하강 사다리. macOS는 hard limit을 `RLIM_INFINITY`로 보고하지만 실제 상한은
/// `kern.maxfilesperproc`이고, 그 위의 값으로 `setrlimit`하면 `EINVAL`이다. sysctl을 따로
/// 읽는 대신 큰 값부터 내려가며 시도해 커널이 받아주는 첫 값을 취한다.
/// 92160은 관측된 `kern.maxfilesperproc` 기본값이다.
const LADDER: [u64; 4] = [92_160, 65_536, 24_576, 10_240];

/// 시도할 soft limit 후보를 큰 것부터 반환한다.
///
/// hard가 유한하면 그 값이 최선이자 상한이므로 맨 앞에 두고 초과 후보는 버린다.
/// 현재 soft 이하는 내리는 셈이라 제외한다 — 후보가 비면 손대지 않는다는 뜻이다.
pub fn candidates(current_soft: u64, hard: u64, infinity: u64) -> Vec<u64> {
    let bounded = hard != infinity;
    let mut out: Vec<u64> = Vec::new();
    if bounded {
        out.push(hard);
    }
    out.extend(LADDER);
    out.retain(|&c| c > current_soft && (!bounded || c <= hard));
    out.sort_unstable_by(|a, b| b.cmp(a));
    out.dedup();
    out
}

/// 상향 시도의 결과. 시작 로그로 남겨 실제 한도를 사후에 확인할 수 있게 한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 이미 충분해 손대지 않았다.
    AlreadySufficient { soft: u64 },
    /// 상향했다.
    Raised { from: u64, to: u64 },
    /// 후보를 모두 시도했으나 커널이 받지 않았다. 기존 soft로 계속 진행한다.
    Failed { soft: u64, reason: String },
    /// unix가 아니라 해당 없음.
    Unsupported,
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::AlreadySufficient { soft } => {
                write!(f, "fd 한도 {soft} — 이미 충분해 그대로 둔다")
            }
            Outcome::Raised { from, to } => write!(f, "fd 한도 {from} → {to} 상향"),
            Outcome::Failed { soft, reason } => {
                write!(f, "fd 한도 상향 실패({reason}) — {soft} 그대로 진행")
            }
            Outcome::Unsupported => write!(f, "fd 한도 보강 해당 없음(비 unix)"),
        }
    }
}

/// `RLIMIT_NOFILE`의 soft를 커널이 허용하는 최대치까지 올린다.
///
/// 실패해도 앱은 계속 뜬다 — 한도가 낮으면 동시 세션이 겹칠 때 spawn이 실패할 뿐,
/// 시작 자체를 막을 이유는 없다.
#[cfg(unix)]
pub fn raise_file_limit() -> Outcome {
    use nix::sys::resource::{getrlimit, setrlimit, Resource, RLIM_INFINITY};

    let (soft, hard) = match getrlimit(Resource::RLIMIT_NOFILE) {
        Ok(pair) => pair,
        Err(error) => {
            return Outcome::Failed {
                soft: 0,
                reason: format!("getrlimit: {error}"),
            }
        }
    };
    let targets = candidates(soft, hard, RLIM_INFINITY);
    if targets.is_empty() {
        return Outcome::AlreadySufficient { soft };
    }

    let mut last = String::new();
    for target in targets {
        // hard는 건드리지 않는다. 낮추면 되돌릴 수 없고, 권한 없이 올릴 수도 없다.
        match setrlimit(Resource::RLIMIT_NOFILE, target as _, hard as _) {
            Ok(()) => {
                return Outcome::Raised {
                    from: soft,
                    to: target,
                }
            }
            Err(error) => last = format!("setrlimit({target}): {error}"),
        }
    }
    Outcome::Failed { soft, reason: last }
}

#[cfg(not(unix))]
pub fn raise_file_limit() -> Outcome {
    Outcome::Unsupported
}

#[cfg(test)]
mod tests {
    use super::*;

    const INF: u64 = u64::MAX;

    #[test]
    fn 무한_hard면_사다리를_큰것부터_시도한다() {
        assert_eq!(
            candidates(256, INF, INF),
            vec![92_160, 65_536, 24_576, 10_240]
        );
    }

    #[test]
    fn 유한_hard는_맨앞이자_상한이다() {
        // hard 30000 — 그보다 큰 사다리 값은 시도할 이유가 없다.
        assert_eq!(candidates(256, 30_000, INF), vec![30_000, 24_576, 10_240]);
    }

    #[test]
    fn 현재보다_낮은_후보는_버린다() {
        // soft 65536이면 그 아래로 내리는 후보는 남지 않는다.
        assert_eq!(candidates(65_536, INF, INF), vec![92_160]);
    }

    #[test]
    fn 이미_최대면_후보가_비어_손대지_않는다() {
        assert!(candidates(92_160, INF, INF).is_empty());
        assert!(candidates(1_048_576, INF, INF).is_empty());
    }

    #[test]
    fn hard와_사다리가_겹쳐도_중복되지_않는다() {
        assert_eq!(candidates(256, 65_536, INF), vec![65_536, 24_576, 10_240]);
    }

    #[test]
    fn hard가_현재_soft와_같으면_올릴_곳이_없다() {
        assert!(candidates(256, 256, INF).is_empty());
    }

    #[test]
    fn 관측된_macos_gui_기본값에서_최대치를_고른다() {
        // launchd 기본 soft 256 / hard unlimited → kern.maxfilesperproc(92160)이 첫 후보.
        let first = candidates(256, INF, INF);
        assert_eq!(first.first(), Some(&92_160));
    }

    #[test]
    fn macos의_실제_무한값에서도_무한으로_취급한다() {
        // macOS의 RLIM_INFINITY는 u64::MAX가 아니라 i64::MAX(0x7fff_ffff_ffff_ffff)다.
        // 실측에서 hard가 이 값으로 보고됐다 — 유한 hard로 오인하면 첫 시도가 헛돈다.
        let macos_inf = i64::MAX as u64;
        assert_eq!(
            candidates(256, macos_inf, macos_inf),
            vec![92_160, 65_536, 24_576, 10_240]
        );
    }

    #[test]
    fn 결과는_사람이_읽을_수_있게_표시된다() {
        assert_eq!(
            Outcome::Raised {
                from: 256,
                to: 92_160
            }
            .to_string(),
            "fd 한도 256 → 92160 상향"
        );
        assert_eq!(
            Outcome::AlreadySufficient { soft: 92_160 }.to_string(),
            "fd 한도 92160 — 이미 충분해 그대로 둔다"
        );
    }
}
