#include <windows.h>

#include <DbgHelp.h>
#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>

#include <cwchar>
#include <string>
#include <vector>

#include "flutter_window.h"
#include "utils.h"

namespace {

std::wstring DiagnosticsDirectory() {
  DWORD length = ::GetEnvironmentVariableW(L"LOCALAPPDATA", nullptr, 0);
  if (length == 0) {
    return L".\\Pure Studio";
  }
  std::vector<wchar_t> buffer(length);
  ::GetEnvironmentVariableW(L"LOCALAPPDATA", buffer.data(), length);
  return std::wstring(buffer.data()) + L"\\Pure Studio";
}

std::wstring CrashDirectory() {
  const std::wstring root = DiagnosticsDirectory();
  ::CreateDirectoryW(root.c_str(), nullptr);
  const std::wstring crashes = root + L"\\crashes";
  ::CreateDirectoryW(crashes.c_str(), nullptr);
  return crashes;
}

std::wstring ExecutableName() {
  std::vector<wchar_t> buffer(32768);
  const DWORD length =
      ::GetModuleFileNameW(nullptr, buffer.data(),
                           static_cast<DWORD>(buffer.size()));
  const std::wstring path(buffer.data(), length);
  const size_t separator = path.find_last_of(L"\\/");
  return separator == std::wstring::npos ? path : path.substr(separator + 1);
}

void ConfigureWindowsErrorReporting() {
  const std::wstring folder = CrashDirectory();
  const std::wstring key_path =
      L"Software\\Microsoft\\Windows\\Windows Error Reporting\\LocalDumps\\" +
      ExecutableName();
  HKEY key = nullptr;
  if (::RegCreateKeyExW(HKEY_CURRENT_USER, key_path.c_str(), 0, nullptr, 0,
                        KEY_SET_VALUE, nullptr, &key, nullptr) !=
      ERROR_SUCCESS) {
    return;
  }
  const DWORD dump_type = 2;
  const DWORD dump_count = 5;
  ::RegSetValueExW(
      key, L"DumpFolder", 0, REG_SZ,
      reinterpret_cast<const BYTE *>(folder.c_str()),
      static_cast<DWORD>((folder.size() + 1) * sizeof(wchar_t)));
  ::RegSetValueExW(key, L"DumpType", 0, REG_DWORD,
                   reinterpret_cast<const BYTE *>(&dump_type),
                   sizeof(dump_type));
  ::RegSetValueExW(key, L"DumpCount", 0, REG_DWORD,
                   reinterpret_cast<const BYTE *>(&dump_count),
                   sizeof(dump_count));
  ::RegCloseKey(key);
}

LONG WINAPI WriteUnhandledMinidump(EXCEPTION_POINTERS *exception) {
  SYSTEMTIME timestamp;
  ::GetSystemTime(&timestamp);
  wchar_t file_name[128];
  ::swprintf_s(file_name,
               L"pure-studio-%04d%02d%02d-%02d%02d%02d-%lu.dmp",
               timestamp.wYear, timestamp.wMonth, timestamp.wDay,
               timestamp.wHour, timestamp.wMinute, timestamp.wSecond,
               ::GetCurrentProcessId());
  const std::wstring path = CrashDirectory() + L"\\" + file_name;
  HANDLE file = ::CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr,
                              CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file != INVALID_HANDLE_VALUE) {
    MINIDUMP_EXCEPTION_INFORMATION exception_info = {
        ::GetCurrentThreadId(), exception, FALSE};
    ::MiniDumpWriteDump(
        ::GetCurrentProcess(), ::GetCurrentProcessId(), file,
        static_cast<MINIDUMP_TYPE>(MiniDumpWithThreadInfo |
                                   MiniDumpWithIndirectlyReferencedMemory),
        exception == nullptr ? nullptr : &exception_info, nullptr, nullptr);
    ::CloseHandle(file);
  }
  return EXCEPTION_EXECUTE_HANDLER;
}

}  // namespace

int APIENTRY wWinMain(_In_ HINSTANCE instance, _In_opt_ HINSTANCE prev,
                      _In_ wchar_t *command_line, _In_ int show_command) {
  ConfigureWindowsErrorReporting();
  ::SetUnhandledExceptionFilter(WriteUnhandledMinidump);

  // Attach to console when present (e.g., 'flutter run') or create a
  // new console when running with a debugger.
  if (!::AttachConsole(ATTACH_PARENT_PROCESS) && ::IsDebuggerPresent()) {
    CreateAndAttachConsole();
  }

  // Initialize COM, so that it is available for use in the library and/or
  // plugins.
  ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

  flutter::DartProject project(L"data");

  std::vector<std::string> command_line_arguments =
      GetCommandLineArguments();

  project.set_dart_entrypoint_arguments(std::move(command_line_arguments));

  FlutterWindow window(project);
  Win32Window::Point origin(10, 10);
  Win32Window::Size size(1280, 720);
  if (!window.Create(L"Pure Studio", origin, size)) {
    return EXIT_FAILURE;
  }
  window.SetQuitOnClose(true);

  ::MSG msg;
  while (::GetMessage(&msg, nullptr, 0, 0)) {
    ::TranslateMessage(&msg);
    ::DispatchMessage(&msg);
  }

  ::CoUninitialize();
  return EXIT_SUCCESS;
}
