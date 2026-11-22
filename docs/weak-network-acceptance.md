# 弱网恢复验收

此脚本验证 sendmer 已提供的持久接收缓存、分块缺失范围恢复和重试策略。桌面端保存恢复摘要，
sendmer Core 负责 manifest 校验、内容寻址缓存、分块校验和重连后的缺失范围请求。

## 环境

Linux 使用专用 network namespace 或 veth 接口。不要在包含其他业务流量的主接口上直接添加
`tc qdisc`。准备两个独立的 sender/receiver 进程，使用同一张票据和独立输出目录。

```bash
sudo ip netns add alter-sendmer-net
sudo ip link add weaknet-host type veth peer name weaknet-peer
sudo ip link set weaknet-peer netns alter-sendmer-net
sudo ip addr add 192.0.2.1/30 dev weaknet-host
sudo ip link set weaknet-host up
sudo ip netns exec alter-sendmer-net ip addr add 192.0.2.2/30 dev weaknet-peer
sudo ip netns exec alter-sendmer-net ip link set weaknet-peer up
```

## 基线矩阵

```bash
scripts/weaknet-baseline.sh --interface weaknet-host --profile latency -- \
  <sender-and-receiver-fixture>
scripts/weaknet-baseline.sh --interface weaknet-host --profile jitter -- \
  <sender-and-receiver-fixture>
scripts/weaknet-baseline.sh --interface weaknet-host --profile loss -- \
  <sender-and-receiver-fixture>
scripts/weaknet-baseline.sh --interface weaknet-host --profile composite \
  --outage-after 10 --outage-seconds 5 --output weaknet-composite.json -- \
  <sender-and-receiver-fixture>
```

测试记录必须包含配置、首次中断时间、恢复时间、重连次数、最后已确认字节、最终 SHA-256、
恢复缓存状态和临时目录清理结果。`150 ms` 往返延迟需要由两端的单向配置合计得到，实验报告
应记录实际测得的 RTT。MacOS Network Link Conditioner 可用于人工 Wi-Fi 切换验收，结果需要
保留同样的字段。

通过条件是恢复后只请求缺失范围，输出文件先完成完整摘要校验再提交，最终 SHA-256 与发送端一致，
成功导出后缓存条目被清理。失败条件包括完整文件摘要不一致、恢复后从零开始且没有记录原因、
临时目录残留或恢复状态与实际事件终态不一致。
