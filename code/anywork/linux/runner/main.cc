#include "my_application.h"

#include "studio_host_lifecycle.h"

int main(int argc, char** argv) {
  g_autoptr(MyApplication) app = my_application_new();
  g_application_run(G_APPLICATION(app), argc, argv);
  // 协调器可靠退出（Dart 侧 finishExit）会在 g_application_run 返回前终止本进程；走到
  // 这里说明 engine/window/消息循环在未获协调器 Clean/NotStarted 结论的情况下结束。
  // 幂等 Arm 让 watchdog 覆盖后续 app 析构、engine teardown 与可能卡住的日志写入，写
  // 最小诊断并返回非 0 —— 绝不允许未经确认的自然 0。
  return anywork::StudioHostLifecycle::Instance().ExitUnconfirmedFromMainLoop();
}
