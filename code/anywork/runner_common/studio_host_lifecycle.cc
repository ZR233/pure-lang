#include "studio_host_lifecycle.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <string>
#include <thread>
#include <utility>

#if defined(_WIN32)
#include <windows.h>
#else
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

namespace {

// 单一总期限：首次 close 之前武装，重复 close 或重试都不延长。
constexpr int64_t kHardDeadlineMs = 30000;
// 最终诊断快照提前量：killer 仍在 30s 触发，诊断在 28s 写，避免与之抢跑。
constexpr int64_t kFinalDiagnosticLeadMs = 2000;
constexpr char kDiagnosticFileName[] = "anywork-exit-diagnostics.log";
constexpr size_t kMaxFieldLength = 128;

#if defined(_WIN32)
using PathString = std::wstring;
#else
using PathString = std::string;
#endif

// 单行字段脱敏：折叠控制字符，截断到有界长度，绝不把换行写进诊断行。
std::string SanitizeField(const std::string& value) {
  std::string out;
  out.reserve(value.size() < kMaxFieldLength ? value.size() : kMaxFieldLength);
  for (char character : value) {
    if (out.size() >= kMaxFieldLength) break;
    const unsigned char code = static_cast<unsigned char>(character);
    out.push_back((code < 0x20 || code == 0x7f) ? '_' : character);
  }
  return out;
}

std::string ProcessIdText() {
#if defined(_WIN32)
  return std::to_string(static_cast<unsigned long long>(::GetCurrentProcessId()));
#else
  return std::to_string(static_cast<unsigned long long>(::getpid()));
#endif
}

int64_t SteadyNowMs() {
  return std::chrono::duration_cast<std::chrono::milliseconds>(
             std::chrono::steady_clock::now().time_since_epoch())
      .count();
}

std::chrono::steady_clock::time_point SteadyAt(int64_t steady_ms) {
  return std::chrono::steady_clock::time_point(
      std::chrono::duration_cast<std::chrono::steady_clock::duration>(
          std::chrono::milliseconds(steady_ms)));
}

// 本进程内的关联标识序号：只用于区分同一进程内的多次武装（正常只有一次），无 IO、无锁。
std::atomic<uint64_t> g_correlation_sequence{0};

// 生成首次武装的、不依赖 Dart 的稳定唯一关联标识：pid + 单调锚点 + 进程内序号。
// 平台无关、无随机源依赖，也不与 Dart 的 `corr-...` 冲突；当 Dart 随后上报有效
// correlationId 时按其既定聚合关系覆盖，否则保留该 native 标识。
std::string NativeCorrelationId(int64_t anchor_steady_ms) {
  const uint64_t sequence = g_correlation_sequence.fetch_add(1);
  std::string id = "corr-native-";
  id += ProcessIdText();
  id += "-";
  id += std::to_string(anchor_steady_ms);
  id += "-";
  id += std::to_string(sequence);
  return id;
}

std::string BuildDiagnosticLine(const char* event,
                                const anywork::StudioExitDiagnostics& diagnostics,
                                int64_t elapsed_ms) {
  std::string line = "anywork-exit pid=";
  line += ProcessIdText();
  line += " event=";
  line += event;
  line += " stage=";
  line += SanitizeField(diagnostics.stage);
  line += " code=";
  line += SanitizeField(diagnostics.code);
  line += " reason=";
  line += SanitizeField(diagnostics.reason);
  line += " correlation=";
  line += SanitizeField(diagnostics.correlation_id);
  line += " pending=";
  line += diagnostics.has_pending_commits
              ? std::to_string(diagnostics.pending_commits)
              : std::string("unknown");
  line += " elapsed_ms=";
  line += std::to_string(elapsed_ms);
  line += "\n";
  return line;
}

// 到期或 finish 时立即结束本进程；不依赖析构、不执行阻塞收尾。
void ForceTerminateImmediately(int exit_code) {
#if defined(_WIN32)
  ::TerminateProcess(::GetCurrentProcess(), static_cast<UINT>(exit_code));
#else
  ::_exit(exit_code);
#endif
}

#if defined(_WIN32)

std::wstring Utf8ToWide(const std::string& value) {
  if (value.empty()) return std::wstring();
  const int size = ::MultiByteToWideChar(
      CP_UTF8, 0, value.c_str(), static_cast<int>(value.size()), nullptr, 0);
  if (size <= 0) return std::wstring();
  std::wstring wide(static_cast<size_t>(size), L'\0');
  const int written =
      ::MultiByteToWideChar(CP_UTF8, 0, value.c_str(),
                            static_cast<int>(value.size()), wide.data(), size);
  if (written <= 0) return std::wstring();
  wide.resize(static_cast<size_t>(written));
  return wide;
}

PathString TempDirectory() {
  wchar_t buffer[MAX_PATH];
  const DWORD length = ::GetTempPathW(MAX_PATH, buffer);
  if (length == 0 || length >= MAX_PATH) return PathString();
  return PathString(buffer, length);
}

PathString DiagnosticFilePath(const PathString& directory) {
  PathString path = directory;
  if (!path.empty() && path.back() != L'\\' && path.back() != L'/') {
    path.push_back(L'\\');
  }
  const std::wstring name = Utf8ToWide(kDiagnosticFileName);
  path.append(name);
  return path;
}

bool WriteFileAt(const PathString& path, const std::string& content) {
  if (path.empty()) return false;
  HANDLE file = ::CreateFileW(path.c_str(), FILE_APPEND_DATA,
                              FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr,
                              OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) return false;
  DWORD written = 0;
  const BOOL ok = ::WriteFile(file, content.data(),
                              static_cast<DWORD>(content.size()), &written,
                              nullptr);
  ::CloseHandle(file);
  return ok != FALSE && written == static_cast<DWORD>(content.size());
}

void ReportFallback(const std::string& line) {
  ::OutputDebugStringA(line.c_str());
  std::fputs(line.c_str(), stderr);
}

#else  // POSIX

PathString TempDirectory() {
  const char* temp = std::getenv("TMPDIR");
  if (temp != nullptr && temp[0] != '\0') return PathString(temp);
  return PathString("/tmp");
}

PathString DiagnosticFilePath(const PathString& directory) {
  PathString path = directory;
  if (!path.empty() && path.back() != '/') path.push_back('/');
  path.append(kDiagnosticFileName);
  return path;
}

bool WriteFileAt(const PathString& path, const std::string& content) {
  if (path.empty()) return false;
  // O_NOFOLLOW + 0600：绝不跟随符号链接写到别处，也不给其它用户读权限。诊断写盘
  // 是尽力而为，任何失败都只回落到 temp / stderr。
  const int fd = ::open(path.c_str(),
                        O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC | O_NOFOLLOW,
                        0600);
  if (fd < 0) return false;
  size_t offset = 0;
  while (offset < content.size()) {
    const ssize_t written =
        ::write(fd, content.data() + offset, content.size() - offset);
    if (written <= 0) {
      ::close(fd);
      return false;
    }
    offset += static_cast<size_t>(written);
  }
  ::close(fd);
  return true;
}

void ReportFallback(const std::string& line) {
  std::fputs(line.c_str(), stderr);
}

#endif

}  // namespace

namespace anywork {

StudioHostLifecycle& StudioHostLifecycle::Instance() {
  // 进程级单例，故意不析构：期限线程 detached，生命周期覆盖整个进程。
  static StudioHostLifecycle* instance = new StudioHostLifecycle();
  return *instance;
}

StudioHostLifecycle::StudioHostLifecycle() = default;

int64_t StudioHostLifecycle::Remaining() const {
  const int64_t deadline =
      deadline_steady_ms_.load(std::memory_order_acquire);
  if (deadline <= 0) return kHardDeadlineMs;
  const int64_t now = SteadyNowMs();
  if (now >= deadline) return 0;
  return deadline - now;
}

int64_t StudioHostLifecycle::Arm() {
  int64_t deadline = 0;
  {
    std::lock_guard<std::mutex> lock(state_mutex_);
    if (armed_) return Remaining();
    armed_ = true;
    // 唯一绝对 deadline：从这一刻起算 30s，重复调用只返回剩余，绝不刷新。
    const int64_t anchor = SteadyNowMs();
    deadline = anchor + kHardDeadlineMs;
    deadline_steady_ms_.store(deadline, std::memory_order_release);
    // 首次武装：生成本进程退出唯一的、不依赖 Dart 的关联标识，并作为 last_ 初始事实。
    // 这样即使随后 Dart 上报的更新缺省 code/correlation，也不会被清空；Dart 一旦给出有效
    // correlationId，则按其既定聚合关系覆盖该 native 标识。
    last_ = StudioExitDiagnostics{};
    last_.stage = "unknown";
    last_.code = "exit-armed";
    // reason 与 stage/code 一样是单行 key=value 里的单个 token，不含空格，便于机器解析；
    // 只使用固定安全文案，绝不携带正文或凭据。
    last_.reason = "native_exit_armed_awaiting_coordinated_finish";
    last_.correlation_id = NativeCorrelationId(anchor);
  }
  // 期限线程：不取任何锁、不做阻塞 IO；sleep_until 同一绝对 deadline 后强制结束。
  std::thread([deadline]() {
    std::this_thread::sleep_until(SteadyAt(deadline));
    ForceTerminateImmediately(1);
  }).detach();
  // 诊断线程：立即写下最小 initial 行（Dart 未响应/engine 已销毁也不至于全无日志），
  // 并在 killer 到期前 2s（28s）写 final 快照，从而不与 30s killer 抢跑；killer 仍
  // sleep_until 30s。它可能先被期限线程终止，这是可接受的尽力而为。
  std::thread([deadline]() {
    anywork::StudioHostLifecycle& self = anywork::StudioHostLifecycle::Instance();
    anywork::StudioExitDiagnostics initial;
    {
      std::lock_guard<std::mutex> lock(self.state_mutex_);
      initial = self.last_;
    }
    self.EmitDiagnostic("initial", initial, 0);
    std::this_thread::sleep_until(SteadyAt(deadline - kFinalDiagnosticLeadMs));
    anywork::StudioExitDiagnostics last;
    {
      std::lock_guard<std::mutex> lock(self.state_mutex_);
      last = self.last_;
    }
    // 28s 仍未真实 finish：明确给出即将到期限的错误码与原因，保留最近阶段、correlation 与
    // 真实或未知的待保存状态。只写本地快照、不回写 last_，避免覆盖其后的真实 finish 诊断。
    last.code = "exitDeadlineImminent";
    last.reason = "native_exit_deadline_imminent_coordinated_finish_missing";
    self.EmitDiagnostic("final", last, kHardDeadlineMs - kFinalDiagnosticLeadMs);
  }).detach();
  return kHardDeadlineMs;
}

void StudioHostLifecycle::UpdateDiagnostics(
    const StudioExitDiagnostics& diagnostics) {
  StudioExitDiagnostics merged;
  {
    std::lock_guard<std::mutex> lock(state_mutex_);
    // 合并而非覆盖：没有有效值的字段保留既有事实，绝不因缺省值清空首次武装生成的
    // correlation / code / reason。Dart 给出的有效 correlation/code/reason 按既定聚合关系
    // 覆盖 native 标识；pendingCommits 只有在明确给出时才更新，缺省则保留最近一次真实值，
    // 不编造保存成功、也不把已确认的排空状态改回未知。
    merged = last_;
    if (!diagnostics.stage.empty()) merged.stage = diagnostics.stage;
    if (!diagnostics.code.empty()) merged.code = diagnostics.code;
    if (!diagnostics.reason.empty()) merged.reason = diagnostics.reason;
    if (!diagnostics.correlation_id.empty()) {
      merged.correlation_id = diagnostics.correlation_id;
    }
    if (diagnostics.has_pending_commits) {
      merged.has_pending_commits = true;
      merged.pending_commits = diagnostics.pending_commits;
    }
    last_ = merged;
  }
  EmitDiagnostic("update", merged, kHardDeadlineMs - Remaining());
}

void StudioHostLifecycle::ConfigureDiagnostics(const std::string& directory) {
  std::lock_guard<std::mutex> lock(log_mutex_);
  configured_directory_ = directory;
}

void StudioHostLifecycle::EmitDiagnostic(
    const char* event, const StudioExitDiagnostics& diagnostics,
    int64_t elapsed_ms) {
  const std::string line = BuildDiagnosticLine(event, diagnostics, elapsed_ms);
  std::lock_guard<std::mutex> lock(log_mutex_);
  // 1) canonical 日志目录（若已由 Dart 提前配置）。
  if (!configured_directory_.empty()) {
    const PathString configured = DiagnosticFilePath(
        // 配置目录是 UTF-8；Windows 下转换为宽字符后写盘。
#if defined(_WIN32)
        Utf8ToWide(configured_directory_)
#else
        configured_directory_
#endif
    );
    if (WriteFileAt(configured, line)) return;
  }
  // 2) 系统 temp 文件。
  const PathString temp = DiagnosticFilePath(TempDirectory());
  if (WriteFileAt(temp, line)) return;
  // 3) stderr（Windows 另有 debug 输出）兜底。
  ReportFallback(line);
}

void StudioHostLifecycle::Finish(int exit_code) {
  // 只接受 0/1；异常退出码一律映射为 1，绝不把未知值透传成进程状态。
  if (exit_code != 0 && exit_code != 1) exit_code = 1;
  StudioExitDiagnostics last;
  {
    std::lock_guard<std::mutex> lock(state_mutex_);
    last = last_;
  }
  // 以真实退出码给出明确 code / 原因，但不覆盖 Dart 已上报的具体错误码（既定聚合关系）：
  // 只有仍是武装占位或为空时才归为 clean/degraded，绝不编造保存成功。
  const bool armed_placeholder =
      last.code.empty() || last.code == "exit-armed";
  if (armed_placeholder) {
    last.code = exit_code == 0 ? "cleanExit" : "degradedExit";
  }
  if (last.reason.empty() ||
      last.reason == "native_exit_armed_awaiting_coordinated_finish") {
    last.reason = exit_code == 0
                      ? "coordinated_shutdown_clean"
                      : "coordinated_shutdown_degraded";
  }
  EmitDiagnostic("finish", last, kHardDeadlineMs - Remaining());
  ForceTerminateImmediately(exit_code);
}

void StudioHostLifecycle::SetExitRequestCallback(ExitRequestCallback callback) {
  std::lock_guard<std::mutex> lock(state_mutex_);
  exit_request_callback_ = std::move(callback);
}

void StudioHostLifecycle::ClearExitRequestCallback() {
  std::lock_guard<std::mutex> lock(state_mutex_);
  exit_request_callback_ = nullptr;
}

void StudioHostLifecycle::RequestExit() {
  // 持续武装：窗口关闭、engine/window 析构都覆盖，绝不提前 disarm。
  // Arm() 只取 state_mutex_，与可能卡住的日志锁无关。
  Arm();
  ExitRequestCallback callback;
  {
    std::lock_guard<std::mutex> lock(state_mutex_);
    callback = exit_request_callback_;
  }
  if (callback) callback();
}

int StudioHostLifecycle::ExitUnconfirmedFromMainLoop() {
  // 幂等武装：已武装只返回剩余、绝不刷新或解除，确保 watchdog 覆盖后续 engine/window
  // 析构、COM teardown 与可能卡住的日志写入；也覆盖此前从未 Arm 过（例如外部 WM_QUIT）
  // 的自然循环退出。不建立第二条期限、不重置既有期限。
  Arm();
  StudioExitDiagnostics diagnostics;
  {
    std::lock_guard<std::mutex> lock(state_mutex_);
    diagnostics = last_;
  }
  // 最小脱敏诊断：保留已知 correlation 与 pending 事实，标注这是未经协调器确认的消息
  // 循环退出。同步写盘可能被卡住的日志锁阻塞，但 killer 线程不取锁、不做 IO，仍会在
  // 30s 到期强制以 1 结束本进程。
  diagnostics.stage = "main";
  diagnostics.code = "loopExitedUnconfirmed";
  diagnostics.reason = "message_loop_exited_uncoordinated";
  UpdateDiagnostics(diagnostics);
  // 非 0：未经协调器 Clean/NotStarted 确认，绝不允许自然返回 0。
  return 1;
}

}  // namespace anywork
