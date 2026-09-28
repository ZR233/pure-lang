#include "flutter_window.h"

#include <windows.h>

#include <shellapi.h>
#include <shlwapi.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <flutter/method_channel.h>
#include <flutter/standard_method_codec.h>

#include <cstdint>
#include <limits>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "flutter/generated_plugin_registrant.h"

namespace {

// Channel and method shared with the Dart host (see vscode_icon.dart).
constexpr char kApplicationIconChannel[] =
    "io.github.zr233.anywork/application_icon";
constexpr char kVsCodeIconMethod[] = "vsCodeIcon";
constexpr char kVsCodeAvailableMethod[] = "vsCodeAvailable";

// Looks up the executable Windows associates with the `vscode` URL protocol.
// Returns false when there is no reliable association, so callers never guess
// an installation path.
bool QueryVsCodeExecutable(std::wstring* executable) {
  DWORD length = 0;
  HRESULT result =
      AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, L"vscode",
                        nullptr, nullptr, &length);
  if (result != S_FALSE || length == 0) {
    return false;
  }
  std::wstring buffer(length, L'\0');
  result = AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, L"vscode",
                             nullptr, buffer.data(), &length);
  if (FAILED(result)) {
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

  application_icon_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(), kApplicationIconChannel,
          &flutter::StandardMethodCodec::GetInstance());
  application_icon_channel_->SetMethodCallHandler(
      [](const flutter::MethodCall<flutter::EncodableValue>& call,
         std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result) {
        if (call.method_name() == kVsCodeAvailableMethod) {
          std::wstring executable;
          result->Success(
              flutter::EncodableValue(QueryVsCodeExecutable(&executable)));
          return;
        }
        if (call.method_name() != kVsCodeIconMethod) {
          result->NotImplemented();
          return;
        }
        const auto png = LoadVsCodeIconPng();
        if (png.empty()) {
          // No protocol association or no icon: report null, not an error.
          result->Success();
          return;
        }
        result->Success(flutter::EncodableValue(png));
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
  if (application_icon_channel_) {
    // The channel does not unregister its handler on destruction, so drop it
    // before the engine goes away.
    application_icon_channel_->SetMethodCallHandler(nullptr);
    application_icon_channel_.reset();
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
