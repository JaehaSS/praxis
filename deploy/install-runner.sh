#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 --repository-root <absolute-path>"
}

repository_root=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --repository-root)
      repository_root="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
done

bundle_dir="$(cd "$(dirname "$0")" && pwd -P)"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/praxis"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/praxis"
bin_dir="$HOME/.local/bin"
config_path="$config_dir/runner.toml"
token_path="$config_dir/pairing.token"
unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

for command in install openssl systemctl; do
  command -v "$command" >/dev/null || {
    echo "Required command is missing: $command" >&2
    exit 1
  }
done

[ -x "$bundle_dir/praxis-runner" ] || {
  echo "praxis-runner is missing from the release bundle" >&2
  exit 1
}

if [ ! -f "$config_path" ]; then
  [ -n "$repository_root" ] || {
    echo "--repository-root is required for the first installation" >&2
    exit 2
  }
  [ -d "$repository_root" ] || {
    echo "Repository root does not exist: $repository_root" >&2
    exit 2
  }
  repository_root="$(cd "$repository_root" && pwd -P)"
fi

toml_escape() {
  sed 's/\\/\\\\/g; s/"/\\"/g' <<<"$1"
}

install -d -m 700 "$config_dir" "$data_dir" "$bin_dir" "$unit_dir"
install -m 755 "$bundle_dir/praxis-runner" "$bin_dir/praxis-runner"
install -m 644 "$bundle_dir/praxis-runner.service" "$unit_dir/praxis-runner.service"

if [ ! -f "$token_path" ]; then
  umask 077
  openssl rand -hex 32 > "$token_path"
  chmod 600 "$token_path"
fi

if [ ! -f "$config_path" ]; then
  escaped_root="$(toml_escape "$repository_root")"
  escaped_token="$(toml_escape "$token_path")"
  cat > "$config_path" <<EOF
bind = "127.0.0.1:47831"
repository_roots = ["$escaped_root"]
max_concurrent_tasks = 2
execution_policy = "always_approve"
pairing_token_file = "$escaped_token"
EOF
  chmod 600 "$config_path"
fi

# linger가 없으면 SSH 세션이 전부 끊길 때 systemd user manager가 내려가면서 Runner도
# 함께 죽는다 — 원격에서 붙는 서비스이므로 로그인 여부와 무관하게 살아 있어야 한다.
# 자기 자신에 대한 enable-linger는 보통 sudo 없이 통하지만, 막힌 환경에서는 설치를
# 실패시키지 않고 안내만 남긴다.
if command -v loginctl >/dev/null; then
  if loginctl enable-linger "$(id -un)" 2>/dev/null; then
    :
  else
    echo "warning: linger를 켜지 못했습니다. SSH 세션이 모두 끊기면 Runner가 종료됩니다." >&2
    echo "         해결: sudo loginctl enable-linger $(id -un)" >&2
  fi
fi

systemctl --user daemon-reload
systemctl --user enable --now praxis-runner

echo "Runner installed: $bin_dir/praxis-runner"
echo "Config preserved at: $config_path"
echo "Pairing token (keep private): $token_path"
# 로그아웃 후에도 살아남는지는 이 값 하나로 갈린다 — 설치 결과와 함께 눈에 보이게 둔다.
echo "Session persistence: $(loginctl show-user "$(id -un)" -p Linger --value 2>/dev/null || echo unknown)"
echo "Verify with: systemctl --user status praxis-runner"
