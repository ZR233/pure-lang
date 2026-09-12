cmake_minimum_required(VERSION 3.13)
project(runner LANGUAGES CXX)

find_program(GLIB_COMPILE_RESOURCES glib-compile-resources REQUIRED)
set(ANYWORK_ICON_DIR "${CMAKE_SOURCE_DIR}/../../../design/assets")
set(ANYWORK_RESOURCE_SOURCE "${CMAKE_CURRENT_BINARY_DIR}/anywork-resources.cc")
add_custom_command(
  OUTPUT "${ANYWORK_RESOURCE_SOURCE}"
  COMMAND "${GLIB_COMPILE_RESOURCES}"
    "${CMAKE_CURRENT_SOURCE_DIR}/anywork.gresource.xml"
    --sourcedir=${ANYWORK_ICON_DIR}
    --target=${ANYWORK_RESOURCE_SOURCE}
    --generate-source --c-name=anywork
  DEPENDS "anywork.gresource.xml" "${ANYWORK_ICON_DIR}/anywork-app-icon.png"
  VERBATIM
)

# Define the application target. To change its name, change BINARY_NAME in the
# top-level CMakeLists.txt, not the value here, or `flutter run` will no longer
# work.
#
# Any new source files that you add to the application should be added here.
add_executable(${BINARY_NAME}
  "main.cc"
  "my_application.cc"
  "${ANYWORK_RESOURCE_SOURCE}"
  "${FLUTTER_MANAGED_DIR}/generated_plugin_registrant.cc"
)

# Apply the standard set of build settings. This can be removed for applications
# that need different build settings.
apply_standard_settings(${BINARY_NAME})

# Add preprocessor definitions for the application ID.
add_definitions(-DAPPLICATION_ID="${APPLICATION_ID}")

# Add dependency libraries. Add any application-specific dependencies here.
target_link_libraries(${BINARY_NAME} PRIVATE flutter)
target_link_libraries(${BINARY_NAME} PRIVATE PkgConfig::GTK)

target_include_directories(${BINARY_NAME} PRIVATE "${CMAKE_SOURCE_DIR}")
