#ifndef RUNNER_COMMON_STUDIO_HOST_LIFECYCLE_H_
#define RUNNER_COMMON_STUDIO_HOST_LIFECYCLE_H_

#include <atomic>
#include <chrono>
#include <cstdint>
#include <functional>
#include <mutex>
#include <string>

namespace anywork {

// 应用退出的一小段脱敏诊断。只承载 stage/code/reason/correlationId/pending 这些非正文、
// 非凭据字段，供退出错误在同步日志、临时应急文件与 stderr 兜底落盘。
struct StudioExitDiagnostics {
  std::string stage;
  std::string code;
  // 与 code 配套的人类可读原因；只使用固定安全文案，绝不携带正文或凭据。
  std::string reason;
  std::string correlation_id;
  bool has_pending_commits = false;
  uint64_t pending_commits = 0;
};

// 原生宿主退出期限与诊断协调器（进程级单例）。
//
// 责任边界（见 design/18 §18.5、design/19）：
//  - Arm() 首次武装 30s 硬期限并返回剩余毫秒；重复调用只返回剩余，绝不刷新。
//    首次武装同时生成本进程退出唯一的、不依赖 Dart 的关联标识并作为 last_ 初始事实，
//    使空字段的诊断更新不会清空既有 correlation/code/reason。
//  - 硬期限由独立线程推进，不依赖 Dart、bridge 或平台消息循环；到期强制结束本进程
//    并以退出码 1。
//  - 期限线程不取任何锁、不做阻塞 IO：到期只调用 ForceTerminate。
//  - 武装时另起一个尽力而为的诊断线程：立即写下最小 initial 行，并在 killer 到期前
//    2s 写 final 快照，因此窗口关闭且 Dart 已失联时仍留有日志，且不与其后的 30s
//    killer 抢跑；final 行显式给出即将到期限的错误码与原因、PID/耗时/最近阶段/真实或
//    未知待保存状态。诊断线程与期限线程互不阻塞。
//  - 状态锁（武装/期限/最后快照）与日志锁分离：卡住的日志写入不会堵住
//    RequestExit()->Arm()。
//  - UpdateDiagnostics 只记录小字段并尽力写一行：空字段保留既有值（Dart 已给出的有效
//    correlation/code 优先），绝不因缺省值清空关键诊断；ConfigureDiagnostics 提前给出
//    canonical 日志目录；Finish 以真实退出码结束本进程。
//  - ExitUnconfirmedFromMainLoop 供平台 main() 在消息循环自然返回后调用：只有协调器
//    可靠 Clean/NotStarted 才会以 0 结束；任何未经协调器确认的 engine/window/loop
//    结束都幂等武装期限、写最小诊断并返回非 0 退出码。
//
// 生命周期：进程级单例，句柄在进程退出前一直保留；期限线程 detached，与其不共享
// 可变状态，因此无需析构同步。持续武装覆盖 engine/window 析构，绝不提前 disarm。
class StudioHostLifecycle {
 public:
  static StudioHostLifecycle& Instance();

  // native 窗口关闭时回调 Dart 侧的同一退出协调器。回调在平台线程被同步调用。
  using ExitRequestCallback = std::function<void()>;

  void SetExitRequestCallback(ExitRequestCallback callback);
  void ClearExitRequestCallback();

  // 首次武装硬期限并返回剩余毫秒；重复调用不刷新。
  int64_t Arm();

  void UpdateDiagnostics(const StudioExitDiagnostics& diagnostics);
  void ConfigureDiagnostics(const std::string& directory);

  // 以给定退出码结束本进程（不返回）。
  void Finish(int exit_code);

  // native 窗口关闭入口：武装期限并请求 Dart 协调器（由 Dart 驱动收束与 finishExit）。
  void RequestExit();

  // 平台 main() 的消息循环自然结束后调用。协调器可靠退出（Finish）会在循环返回前终止
  // 本进程，因此走到这里说明 engine/window/消息循环在未获协调器 Clean/NotStarted 结论
  // 的情况下结束——unconfirmed 退出，绝不允许自然返回 0。
  //
  // 基于唯一 host 事实源：幂等 Arm（已武装只返回剩余，绝不重置/解除）确保 watchdog 覆盖
  // 后续析构 / COM teardown / 日志卡住；写下最小脱敏诊断；返回非 0 退出码（1）。
  // 只返回退出码、不在此强制终止，让平台正常析构在 watchdog 保护下完成。
  int ExitUnconfirmedFromMainLoop();

 private:
  StudioHostLifecycle();
  StudioHostLifecycle(const StudioHostLifecycle&) = delete;
  StudioHostLifecycle& operator=(const StudioHostLifecycle&) = delete;

  // 期限剩余毫秒；不取锁，读取两个原子量。
  int64_t Remaining() const;
  // 组装并落盘一行诊断；内部取 log_mutex_，绝不与 state_mutex_ 嵌套。
  void EmitDiagnostic(const char* event,
                      const StudioExitDiagnostics& diagnostics,
                      int64_t elapsed_ms);

  // 状态锁：只保护 last_ 与回调，绝不包裹 IO。
  std::mutex state_mutex_;
  // 日志锁：只保护 configured_directory_ 与文件写入，卡住也不影响 Arm/期限线程。
  std::mutex log_mutex_;
  bool armed_ = false;
  std::atomic<int64_t> deadline_steady_ms_{0};
  StudioExitDiagnostics last_;
  std::string configured_directory_;
  ExitRequestCallback exit_request_callback_;
};

}  // namespace anywork

#endif  // RUNNER_COMMON_STUDIO_HOST_LIFECYCLE_H_
