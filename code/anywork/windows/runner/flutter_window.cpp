#include "flutter_window.h"

#include <windows.h>

#include <shlwapi.h>
#include <shellapi.h>
#include <shlobj.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <cstdint>
#include <limits>
#include <vector>

#include <flutter/method_channel.h>
#include <flutter/standard_method_codec.h>

#include <map>
#include <memory>
#include <optional>
#include <string>
#include <variant>

#include "flutter/generated_plugin_registrant.h"
#include "studio_host_lifecycle.h"
#include "utils.h"

namespace {

// Shared with host_app_icons.dart and the desktop application launchers.
constexpr char kHostAppsChannel[] = "io.github.zr233.anywork/host_apps";
// Shared with lib/src/app/studio_host_lifecycle.dart (native exit contract).
constexpr char kHostLifecycleChannel[] =
    "io.github.zr233.anywork/host_lifecycle";
constexpr char kVsCodeExecutableMethod[] = "vsCodeExecutable";
constexpr char kZedExecutableMethod[] = "zedExecutable";

// Reads an optional string field from a channel argument map.
std::string StringField(const flutter::EncodableMap& arguments,
                        const char* key) {
  const auto entry =
      arguments.find(flutter::EncodableValue(std::string(key)));
  if (entry == arguments.end()) return std::string();
  if (const auto* value = std::get_if<std::string>(&entry->second)) {
    return *value;
  }
  return std::string();
}

// Handles the native-host lifecycle method channel. Every method stays on the
// platform thread; diagnostics writes never touch the independent deadline
// thread, and finishExit terminates this instance without returning.
void HandleHostLifecycleCall(
    const flutter::MethodCall<flutter::EncodableValue>& call,
    std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result) {
  anywork::StudioHostLifecycle& lifecycle =
      anywork::StudioHostLifecycle::Instance();
  const std::string& method = call.method_name();
  if (method == "beginExit") {
    result->Success(flutter::EncodableValue(
        static_cast<int64_t>(lifecycle.Arm())));
    return;
  }
  if (method == "updateExitDiagnostics") {
    anywork::StudioExitDiagnostics diagnostics;
    if (const auto* arguments =
            std::get_if<flutter::EncodableMap>(call.arguments())) {
      diagnostics.stage = StringField(*arguments, "stage");
      diagnostics.code = StringField(*arguments, "code");
      diagnostics.correlation_id = StringField(*arguments, "correlationId");
      const auto pending = arguments->find(
          flutter::EncodableValue(std::string("pendingCommits")));
      if (pending != arguments->end()) {
        if (const auto* count =
                std::get_if<int64_t>(&pending->second)) {
          diagnostics.has_pending_commits = true;
          diagnostics.pending_commits = static_cast<uint64_t>(*count);
        } else if (const auto* small =
                       std::get_if<int32_t>(&pending->second)) {
          diagnostics.has_pending_commits = true;
          diagnostics.pending_commits = static_cast<uint64_t>(*small);
        }
      }
    }
    lifecycle.UpdateDiagnostics(diagnostics);
    result->Success();
    return;
  }
  if (method == "configureDiagnostics") {
    std::string directory;
    if (const auto* arguments =
            std::get_if<flutter::EncodableMap>(call.arguments())) {
      directory = StringField(*arguments, "directory");
    }
    lifecycle.ConfigureDiagnostics(directory);
    result->Success();
    return;
  }
  if (method == "finishExit") {
    // 缺失或异常退出码一律按失败处理；native Finish 只接受 0/1。
    int exit_code = 1;
    if (const auto* arguments =
            std::get_if<flutter::EncodableMap>(call.arguments())) {
      const auto value =
          arguments->find(flutter::EncodableValue(std::string("exitCode")));
      if (value != arguments->end()) {
        if (const auto* code = std::get_if<int32_t>(&value->second);
            code != nullptr && (*code == 0 || *code == 1)) {
          exit_code = *code;
        } else if (const auto* code64 = std::get_if<int64_t>(&value->second);
                   code64 != nullptr && (*code64 == 0 || *code64 == 1)) {
          exit_code = static_cast<int>(*code64);
        }
      }
    }
    lifecycle.Finish(exit_code);
    result->Success();
    return;
  }
  result->NotImplemented();
}

// Looks up the executable Windows associates with a URL protocol.
bool QueryProtocolExecutable(const wchar_t* protocol,
                             std::wstring* executable) {
  DWORD length = 0;
  HRESULT result =
      AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, protocol,
                        nullptr, nullptr, &length);
  if (result != S_FALSE || length == 0) {
    return false;
  }
  std::wstring buffer(length, L'\0');
  result = AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, protocol,
                             nullptr, buffer.data(), &length);
  if (FAILED(result)) {
    return false;
  }
  *executable = std::wstring(buffer.c_str());
  return !executable->empty();
}

// Availability probes and launches share the registered VS Code executable.
bool QueryVsCodeExecutable(std::wstring* executable) {
  return QueryProtocolExecutable(L"vscode", executable);
}

// Zed's URL protocol points at the graphical application, while its official
// command-line program is installed separately on PATH. Never substitute the
// protocol executable because it does not implement the same argv contract.
bool QueryZedExecutable(std::wstring* executable) {
  const DWORD length =
      SearchPathW(nullptr, L"zed.exe", nullptr, 0, nullptr, nullptr);
  if (length == 0) {
    return false;
  }
  std::wstring buffer(length, L'\0');
  const DWORD copied = SearchPathW(nullptr, L"zed.exe", nullptr, length,
                                   buffer.data(), nullptr);
  if (copied == 0 || copied >= length) {
    return false;
  }
  *executable = std::wstring(buffer.c_str());
  return !executable->empty();
}

// WIC preserves icon alpha and handles legacy masks. COM is initialized by
// wWinMain; every interface below is released before the method call returns.
std::vector<uint8_t> EncodeIconAsPng(HICON icon) {
  using Microsoft::WRL::ComPtr;
  ComPtr<IWICImagingFactory> factory;
  ComPtr<IWICBitmap> bitmap;
  ComPtr<IStream> stream;
  ComPtr<IWICBitmapEncoder> encoder;
  ComPtr<IWICBitmapFrameEncode> frame;
  if (FAILED(CoCreateInstance(CLSID_WICImagingFactory, nullptr,
                              CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory))) ||
      FAILED(factory->CreateBitmapFromHICON(icon, &bitmap)) ||
      FAILED(CreateStreamOnHGlobal(nullptr, TRUE, &stream)) ||
      FAILED(factory->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder)) ||
      FAILED(encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache)) ||
      FAILED(encoder->CreateNewFrame(&frame, nullptr)) ||
      FAILED(frame->Initialize(nullptr))) {
    return {};
  }
  WICPixelFormatGUID format = GUID_WICPixelFormat32bppBGRA;
  if (FAILED(frame->SetPixelFormat(&format)) ||
      FAILED(frame->WriteSource(bitmap.Get(), nullptr)) ||
      FAILED(frame->Commit()) || FAILED(encoder->Commit())) {
    return {};
  }
  STATSTG stat = {};
  if (FAILED(stream->Stat(&stat, STATFLAG_NONAME)) ||
      stat.cbSize.QuadPart == 0 ||
      stat.cbSize.QuadPart > std::numeric_limits<ULONG>::max() ||
      FAILED(stream->Seek({}, STREAM_SEEK_SET, nullptr))) {
    return {};
  }
  const auto size = static_cast<ULONG>(stat.cbSize.QuadPart);
  std::vector<uint8_t> png(size);
  ULONG read = 0;
  if (FAILED(stream->Read(png.data(), size, &read)) || read != size) {
    return {};
  }
  return png;
}

// Reads the icon of the `vscode` protocol's associated executable. Returns an
// empty vector when there is no association or no icon, so the UI falls back to
// a labeled button instead of any guessed artwork.
std::vector<uint8_t> LoadVsCodeIconPng() {
  std::wstring executable;
  if (!QueryVsCodeExecutable(&executable)) {
    return {};
  }
  SHFILEINFOW file_info = {};
  if (SHGetFileInfoW(executable.c_str(), 0, &file_info, sizeof(file_info),
                     SHGFI_ICON | SHGFI_LARGEICON) == 0 ||
      file_info.hIcon == nullptr) {
    return {};
  }
  auto png = EncodeIconAsPng(file_info.hIcon);
  DestroyIcon(file_info.hIcon);
  return png;
}

// The protocol association is only an application identity used for artwork;
// Zed launches continue to use the independent command-line executable.
std::vector<uint8_t> LoadZedIconPng() {
  std::wstring executable;
  if (!QueryProtocolExecutable(L"zed", &executable)) {
    return {};
  }
  SHFILEINFOW file_info = {};
  if (SHGetFileInfoW(executable.c_str(), 0, &file_info, sizeof(file_info),
                     SHGFI_ICON | SHGFI_LARGEICON) == 0 ||
      file_info.hIcon == nullptr) {
    return {};
  }
  auto png = EncodeIconAsPng(file_info.hIcon);
  DestroyIcon(file_info.hIcon);
  return png;
}

// Execution aliases carry a generic file icon. Ask the shell for the registered
// Terminal application instead; these are application identities, not install paths.
std::vector<uint8_t> LoadTerminalIconPng() {
  for (const auto* app : {
           L"shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
           L"shell:AppsFolder\\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe!App"}) {
    PIDLIST_ABSOLUTE item = nullptr;
    if (FAILED(SHParseDisplayName(app, nullptr, &item, 0, nullptr))) continue;
    SHFILEINFOW info = {};
    const auto found = SHGetFileInfoW(
        reinterpret_cast<LPCWSTR>(item), 0, &info, sizeof(info),
        SHGFI_PIDL | SHGFI_ICON | SHGFI_LARGEICON);
    CoTaskMemFree(item);
    if (found == 0 || info.hIcon == nullptr) continue;
    auto png = EncodeIconAsPng(info.hIcon);
    DestroyIcon(info.hIcon);
    if (!png.empty()) return png;
  }
  return {};
}

}  // namespace

FlutterWindow::FlutterWindow(const flutter::DartProject& project)
    : project_(project) {}

FlutterWindow::~FlutterWindow() {}

bool FlutterWindow::OnCreate() {
  if (!Win32Window::OnCreate()) {
    return false;
  }

  RECT frame = GetClientArea();

  // The size here must match the window dimensions to avoid unnecessary surface
  // creation / destruction in the startup path.
  flutter_controller_ = std::make_unique<flutter::FlutterViewController>(
      frame.right - frame.left, frame.bottom - frame.top, project_);
  // Ensure that basic setup of the controller was successful.
  if (!flutter_controller_->engine() || !flutter_controller_->view()) {
    return false;
  }
  RegisterPlugins(flutter_controller_->engine());

  host_apps_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(), kHostAppsChannel,
          &flutter::StandardMethodCodec::GetInstance());
  host_apps_channel_->SetMethodCallHandler(
      [](const flutter::MethodCall<flutter::EncodableValue>& call,
         std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result) {
        if (call.method_name() == kVsCodeExecutableMethod) {
          std::wstring executable;
          if (QueryVsCodeExecutable(&executable)) {
            result->Success(
                flutter::EncodableValue(Utf8FromUtf16(executable.c_str())));
          } else {
            result->Success();
          }
          return;
        }
        if (call.method_name() == kZedExecutableMethod) {
          std::wstring executable;
          if (QueryZedExecutable(&executable)) {
            result->Success(
                flutter::EncodableValue(Utf8FromUtf16(executable.c_str())));
          } else {
            result->Success();
          }
          return;
        }
        if (call.method_name() == "vsCodeIcon" ||
            call.method_name() == "zedIcon" ||
            call.method_name() == "terminalIcon") {
          const auto png = call.method_name() == "vsCodeIcon"
                               ? LoadVsCodeIconPng()
                               : call.method_name() == "zedIcon"
                                     ? LoadZedIconPng()
                                     : LoadTerminalIconPng();
          if (png.empty()) {
            result->Success();
          } else {
            result->Success(flutter::EncodableValue(png));
          }
          return;
        }
        result->NotImplemented();
      });

  lifecycle_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(), kHostLifecycleChannel,
          &flutter::StandardMethodCodec::GetInstance());
  lifecycle_channel_->SetMethodCallHandler(
      [](const flutter::MethodCall<flutter::EncodableValue>& call,
         std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result) {
        HandleHostLifecycleCall(call, std::move(result));
      });
  // The native close path asks Dart for the same coordinator; the callback stays
  // on the platform thread and is cleared before the channel goes away.
  anywork::StudioHostLifecycle::Instance().SetExitRequestCallback([this]() {
    if (lifecycle_channel_) {
      lifecycle_channel_->InvokeMethod("requestExit", nullptr);
    }
  });

  SetChildContent(flutter_controller_->view()->GetNativeWindow());

  flutter_controller_->engine()->SetNextFrameCallback([&]() {
    this->Show();
  });

  // Flutter can complete the first frame before the "show window" callback is
  // registered. The following call ensures a frame is pending to ensure the
  // window is shown. It is a no-op if the first frame hasn't completed yet.
  flutter_controller_->ForceRedraw();

  return true;
}

void FlutterWindow::OnDestroy() {
  // Clear the native close callback before the channel disappears; the deadline
  // thread keeps running so engine/window teardown cannot disarm the exit bound.
  anywork::StudioHostLifecycle::Instance().ClearExitRequestCallback();
  if (lifecycle_channel_) {
    lifecycle_channel_->SetMethodCallHandler(nullptr);
    lifecycle_channel_.reset();
  }
  if (host_apps_channel_) {
    // The channel does not unregister its handler on destruction, so drop it
    // before the engine goes away.
    host_apps_channel_->SetMethodCallHandler(nullptr);
    host_apps_channel_.reset();
  }
  if (flutter_controller_) {
    flutter_controller_ = nullptr;
  }

  Win32Window::OnDestroy();
}

LRESULT
FlutterWindow::MessageHandler(HWND hwnd, UINT const message,
                              WPARAM const wparam,
                              LPARAM const lparam) noexcept {
  // Intercept the close request before Flutter handles it: arm the native
  // deadline and hand the close back to the same Dart exit coordinator.
  if (message == WM_CLOSE) {
    anywork::StudioHostLifecycle::Instance().RequestExit();
    return 0;
  }
  // Keep the deadline armed across engine/window teardown; never disarm early.
  if (message == WM_DESTROY || message == WM_ENDSESSION ||
      message == WM_QUERYENDSESSION) {
    anywork::StudioHostLifecycle::Instance().Arm();
  }

  // Give Flutter, including plugins, an opportunity to handle window messages.
  if (flutter_controller_) {
    std::optional<LRESULT> result =
        flutter_controller_->HandleTopLevelWindowProc(hwnd, message, wparam,
                                                      lparam);
    if (result) {
      return *result;
    }
  }

  switch (message) {
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}
