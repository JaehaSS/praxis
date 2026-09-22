//! spawn과 wait 사이의 이탈에서 자식을 지키는 가드.
//!
//! `std::process::Child`는 drop돼도 `wait`하지 않는다. spawn 이후 `?`로 빠져나갈 수 있는
//! 경로가 하나라도 있으면 그 자식은 회수되지 않고 좀비로 남는다. 실측에서 이틀에 7개가
//! 쌓였고 전부 이름이 `praxis`였다 — exec 전에 죽은 자식은 부모 이름을 물려받기 때문이다
//! (exec에 성공한 자식의 좀비는 exec된 이름을 단다. 둘을 실측으로 갈랐다).

use std::process::Child;

/// drop 시점에 **아직 살아 있는** 자식만 정리한다.
///
/// 이미 회수된 자식을 건드리지 않는 것이 이 가드의 핵심이다. 정상 경로도 `wait`를 마친 뒤
/// 이 가드를 drop하는데, 거기서 `kill_group`을 부르면 그사이 재사용된 pid를 죽일 수 있다.
/// `try_wait`은 `wait`이 캐시해 둔 종료 상태를 그대로 주므로 이 구분이 공짜다.
pub(super) struct ReapOnDrop(Child);

impl ReapOnDrop {
    pub(super) fn new(child: Child) -> Self {
        Self(child)
    }
}

impl std::ops::Deref for ReapOnDrop {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for ReapOnDrop {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for ReapOnDrop {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return; // 이미 회수됐다.
        }
        crate::verify::kill_group(self.0.id());
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 살아 있는 자식은 가드가 drop되는 것만으로 트리째 정리된다.
    #[test]
    #[cfg(unix)]
    fn drop_reaps_a_live_child() {
        use std::os::unix::process::CommandExt;

        let mut command = std::process::Command::new("sleep");
        command.arg("30").process_group(0); // pgid == pid — kill_group이 닿는 형태.
        let child = command.spawn().expect("sleep 스폰 실패");
        let pid = child.id();
        assert!(
            crate::verify::process_group_alive(pid),
            "스폰 직후엔 살아 있어야 한다"
        );

        drop(ReapOnDrop::new(child));

        assert!(
            !crate::verify::process_group_alive(pid),
            "가드가 drop됐으면 자식이 남아 있으면 안 된다"
        );
    }

    /// 정상 경로가 이미 `wait`한 자식에는 손대지 않는다 — pid 재사용 창을 열지 않기 위해서다.
    #[test]
    fn drop_is_a_noop_for_an_already_reaped_child() {
        let mut child = std::process::Command::new("/bin/echo")
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("echo 스폰 실패");
        child.wait().expect("wait 실패");

        drop(ReapOnDrop::new(child)); // kill 경로로 새지 않아야 한다.
    }
}
