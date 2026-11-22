#!/usr/bin/env bash
set -euo pipefail

# 在专用 Linux network namespace 或 veth 接口上运行。脚本结束时移除 netem 规则。
interface=""
profile="composite"
output="weaknet-result.json"
outage_after=0
outage_seconds=0

usage() {
    cat <<'EOF'
Usage: weaknet-baseline.sh --interface IFACE [options] -- COMMAND [ARGS...]
Profiles: latency, jitter, loss, rate, composite
Options: --profile PROFILE --output FILE --outage-after SECONDS --outage-seconds SECONDS
EOF
}

while (($#)); do
    case "$1" in
        --interface) interface="$2"; shift 2 ;;
        --profile) profile="$2"; shift 2 ;;
        --output) output="$2"; shift 2 ;;
        --outage-after) outage_after="$2"; shift 2 ;;
        --outage-seconds) outage_seconds="$2"; shift 2 ;;
        --) shift; break ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; exit 2 ;;
    esac
done

if [[ -z "$interface" || $# -eq 0 ]]; then
    usage >&2
    exit 2
fi

case "$profile" in
    latency) netem=(delay 150ms) ;;
    jitter) netem=(delay 150ms 30ms distribution normal) ;;
    loss) netem=(loss 1%) ;;
    rate) netem=(rate 5mbit) ;;
    composite) netem=(delay 150ms 30ms distribution normal loss 1% rate 5mbit) ;;
    *) echo "Unsupported profile: $profile" >&2; exit 2 ;;
esac

command -v tc >/dev/null || { echo 'tc is required' >&2; exit 1; }
start_epoch="$(date +%s)"
cleanup() {
    tc qdisc del dev "$interface" root >/dev/null 2>&1 || true
}
trap cleanup EXIT

tc qdisc replace dev "$interface" root netem "${netem[@]}"
set +e
"$@" &
command_pid=$!
if ((outage_seconds > 0)); then
    sleep "$outage_after"
    tc qdisc change dev "$interface" root netem loss 100%
    sleep "$outage_seconds"
    tc qdisc change dev "$interface" root netem "${netem[@]}"
fi
wait "$command_pid"
command_status=$?
set -e
end_epoch="$(date +%s)"

cat > "$output" <<EOF
{"profile":"$profile","interface":"$interface","started_at":$start_epoch,"finished_at":$end_epoch,"command_status":$command_status,"outage_after_seconds":$outage_after,"outage_seconds":$outage_seconds}
EOF
exit "$command_status"
