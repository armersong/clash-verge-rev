#!/bin/bash
# Clash Verge Rev - Rule 模式自动选择最佳服务器
# 用法: ./clash-rule-auto.sh [--once|--daemon [--interval 秒]]

CONTROLLER="http://127.0.0.1:9097"
SECRET="your-secret-here"
TEST_URL="http://cp.cloudflare.com/generate_204"
TIMEOUT=5000
INTERVAL=300

usage() {
    cat << EOF
Clash Verge Rev - Rule 模式自动选择最佳服务器

用法: $0 [选项]

选项:
  --once           单次运行所有 url-test 组自动选优
  --daemon         后台守护模式持续运行
  --interval 秒    守护模式间隔（默认 300 秒）
  --help           显示帮助

示例:
  $0 --once                    # 单次执行
  $0 --daemon                  # 后台运行，每 5 分钟执行
  $0 --daemon --interval 600   # 后台运行，每 10 分钟执行

前提:
  1. Clash Verge Rev 已启动并开启 "允许外部控制器"
  2. 设置正确的 CONTROLLER 和 SECRET
EOF
    exit 0
}

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1"
}

get_url_test_groups() {
    local proxies
    proxies=$(curl -s -X GET "$CONTROLLER/proxies" \
        -H "Authorization: Bearer $SECRET" 2>/dev/null)

    if [ -z "$proxies" ]; then
        log "错误: 无法获取代理列表"
        return 1
    fi

    echo "$proxies" | grep -o '"url-test"' | wc -l | xargs
}

select_best_for_group() {
    local group_name="$1"
    local test_url="${2:-$TEST_URL}"
    local timeout="${3:-$TIMEOUT}"

    local all_proxies now_proxy best_proxy best_delay

    all_proxies=$(curl -s -X GET "$CONTROLLER/proxies/$group_name" \
        -H "Authorization: Bearer $SECRET" | \
        grep -o '"name":"[^"]*"' | sed 's/"name":"//;s/"//g')

    if [ -z "$all_proxies" ]; then
        return 1
    fi

    now_proxy=$(curl -s -X GET "$CONTROLLER/proxies/$group_name" \
        -H "Authorization: Bearer $SECRET" | \
        grep -o '"now":"[^"]*"' | sed 's/"now":"//;s/"//g')

    best_proxy=""
    best_delay=99999

    for proxy in $all_proxies; do
        local delay
        delay=$(curl -s -o /dev/null -w "%{time_total}" \
            --connect-timeout 3 \
            -x "http://$SECRET@127.0.0.1:7890" \
            "$test_url" 2>/dev/null | awk '{print int($1 * 1000)}')

        if [ "$delay" -gt 0 ] && [ "$delay" -lt "$best_delay" ]; then
            best_delay=$delay
            best_proxy=$proxy
        fi
    done

    if [ -n "$best_proxy" ] && [ "$best_proxy" != "$now_proxy" ]; then
        log "切换 $group_name: $now_proxy -> $best_proxy (${best_delay}ms)"
        curl -s -X PUT "$CONTROLLER/proxies/$group_name" \
            -H "Authorization: Bearer $SECRET" \
            -d "{\"name\": \"$best_proxy\"}" > /dev/null
    else
        log "$group_name: 当前 $now_proxy 已是最佳"
    fi
}

auto_select_all() {
    local proxies
    proxies=$(curl -s -X GET "$CONTROLLER/proxies" \
        -H "Authorization: Bearer $SECRET" 2>/dev/null)

    if [ -z "$proxies" ]; then
        log "错误: 无法获取代理列表"
        return 1
    fi

    local groups
    groups=$(echo "$proxies" | grep -oP '"url-test[^"]*"(?=,")' | sed 's/":"/\\"/g' | grep -v "url-test-all" | grep -v "url-test-auto" | sed 's/\\"//g')

    for group in $groups; do
        local type
        type=$(echo "$proxies" | grep -oP "\"$group\":\{[^}]+\}" | grep -oP '"type":"[^"]*"' | sed 's/"type":"//;s/"//g')
        if [ "$type" = "url-test" ]; then
            select_best_for_group "$group"
        fi
    done
}

MODE="once"
while [[ $# -gt 0 ]]; do
    case $1 in
        --once) MODE="once"; shift ;;
        --daemon) MODE="daemon"; shift ;;
        --interval)
            INTERVAL="$2"
            shift 2
            ;;
        --help) usage ;;
        *) shift ;;
    esac
done

log "Clash Verge Rev - 自动选优启动"
log "控制器: $CONTROLLER"

case $MODE in
    once)
        auto_select_all
        ;;
    daemon)
        log "守护模式启动，间隔 ${INTERVAL} 秒"
        while true; do
            auto_select_all
            sleep "$INTERVAL"
        done
        ;;
esac
