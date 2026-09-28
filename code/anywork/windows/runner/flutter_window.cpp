#include "flutter_window.h"

#include <windows.h>

#include <gdiplus.h>
#include <shellapi.h>
#include <shlwapi.h>

#include <flutter/method_channel.h>
#include <flutter/standard_method_codec.h>

#include <cstdint>
#include <cwchar>
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

// Finds the GDI+ encoder CLSID for PNG output.
bool GetPngEncoderClsid(CLSID* clsid) {
  UINT count = 0;
  UINT size = 0;
  if (Gdiplus::GetImageEncodersSize(&count, &size) != Gdiplus::Ok ||
      size == 0) {
    return false;
  }
  std::vector<uint8_t> buffer(size);
  auto* encoders = reinterpret_cast<Gdiplus::ImageCodecInfo*>(buffer.data());
  if (Gdiplus::GetImageEncoders(count, size, encoders) != Gdiplus::Ok) {
    return false;
  }
  for (UINT index = 0; index < count; ++index) {
    if (wcscmp(encoders[index].MimeType, L"image/png") == 0) {
      *clsid = encoders[index].Clsid;
      return true;
    }
  }
  return false;
}

// Encodes a shell icon as PNG bytes. The HICON stays owned by the caller.
std::vector<uint8_t> EncodeIconAsPng(HICON icon, const CLSID& encoder_clsid) {
  std::vector<uint8_t> png;
  Gdiplus::Bitmap bitmap(icon);
  if (bitmap.GetLastStatus() != Gdiplus::Ok) {
    return png;
  }
  IStream* stream = nullptr;
  if (CreateStreamOnHGlobal(nullptr, TRUE, &stream) != S_OK) {
    return png;
  }
  if (bitmap.Save(stream, &encoder_clsid, nullptr) == Gdiplus::Ok) {
    STATSTG stat = {};
    if (stream->Stat(&stat, STATFLAG_NONAME) == S_OK &&
        stat.cbSize.QuadPart > 0) {
      HGLOBAL global = nullptr;
      if (GetHGlobalFromStream(stream, &global) == S_OK && global != nullptr) {
        const auto* data = static_cast<const uint8_t*>(GlobalLock(global));
        if (data != nullptr) {
          const auto size = static_cast<size_t>(stat.cbSize.QuadPart);
          png.assign(data, data + size);
          GlobalUnlock(global);
        }
      }
    }
  }
  stream->Release();
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
  CLSID encoder_clsid;
  std::vector<uint8_t> png;
  if (GetPngEncoderClsid(&encoder_clsid)) {
    png = EncodeIconAsPng(file_info.hIcon, encoder_clsid);
  }
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

  // Start GDI+ once for the window so icon encoding reuses the encoder set.
  Gdiplus::GdiplusStartupInput gdiplus_input;
  if (Gdiplus::GdiplusStartup(&gdiplus_token_, &gdiplus_input, nullptr) !=
      Gdiplus::Ok) {
    gdiplus_token_ = 0;
  }

  application_icon_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(), kApplicationIconChannel,
          &flutter::StandardMethodCodec::GetInstance());
  application_icon_channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
             std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
                 result) {
        if (call.method_name() != kVsCodeIconMethod) {
          result->NotImplemented();
          return;
        }
        // GDI+ failed to start: report no icon instead of calling GDI+ APIs. A
        // null result never blocks application startup or opening the URL.
        std::vector<uint8_t> png;
        if (gdiplus_token_ != 0) {
          png = LoadVsCodeIconPng();
        }
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
    // before the window goes away; the handler captured this window.
    application_icon_channel_->SetMethodCallHandler(nullptr);
    application_icon_channel_.reset();
  }
  if (gdiplus_token_ != 0) {
    Gdiplus::GdiplusShutdown(gdiplus_token_);
    gdiplus_token_ = 0;
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
