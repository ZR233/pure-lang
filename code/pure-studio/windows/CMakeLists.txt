# Project-level configuration.
cmake_minimum_required(VERSION 3.20)
project(pure_studio LANGUAGES CXX)

# The name of the executable created for the application. Change this to change
# the on-disk name of your application.
set(BINARY_NAME "pure_studio")

# Explicitly opt in to modern CMake behaviors to avoid warnings with recent
# versions of CMake.
cmake_policy(VERSION 3.14...3.25)

# Define build configuration option.
get_property(IS_MULTICONFIG GLOBAL PROPERTY GENERATOR_IS_MULTI_CONFIG)
if(IS_MULTICONFIG)
  set(CMAKE_CONFIGURATION_TYPES "Debug;Profile;Release"
    CACHE STRING "" FORCE)
else()
  if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE "Debug" CACHE
      STRING "Flutter build mode" FORCE)
    set_property(CACHE CMAKE_BUILD_TYPE PROPERTY STRINGS
      "Debug" "Profile" "Release")
  endif()
endif()
# Define settings for the Profile build mode.
set(CMAKE_EXE_LINKER_FLAGS_PROFILE "${CMAKE_EXE_LINKER_FLAGS_RELEASE}")
set(CMAKE_SHARED_LINKER_FLAGS_PROFILE "${CMAKE_SHARED_LINKER_FLAGS_RELEASE}")
set(CMAKE_C_FLAGS_PROFILE "${CMAKE_C_FLAGS_RELEASE}")
set(CMAKE_CXX_FLAGS_PROFILE "${CMAKE_CXX_FLAGS_RELEASE}")

# Use Unicode for all projects.
add_definitions(-DUNICODE -D_UNICODE)

# Compilation settings that should be applied to most targets.
#
# Be cautious about adding new options here, as plugins use this function by
# default. In most cases, you should add new options to specific targets instead
# of modifying this function.
function(APPLY_STANDARD_SETTINGS TARGET)
  target_compile_features(${TARGET} PUBLIC cxx_std_17)
  target_compile_options(${TARGET} PRIVATE /W4 /WX /wd"4100")
  target_compile_options(${TARGET} PRIVATE /EHsc)
  target_compile_definitions(${TARGET} PRIVATE "_HAS_EXCEPTIONS=0")
  target_compile_definitions(${TARGET} PRIVATE "$<$<CONFIG:Debug>:_DEBUG>")
endfunction()

# Flutter library and tool build rules.
set(FLUTTER_MANAGED_DIR "${CMAKE_CURRENT_SOURCE_DIR}/flutter")
add_subdirectory(${FLUTTER_MANAGED_DIR})

# Application build; see runner/CMakeLists.txt.
add_subdirectory("runner")

if("$ENV{PURE_STUDIO_DEMO}" STREQUAL "true")
  message(STATUS "PURE_STUDIO_DEMO=true; skipping Rust bridge DLL staging.")
else()
  set(PURE_STUDIO_BRIDGE_LIBRARY "$ENV{PURE_STUDIO_BRIDGE_LIBRARY}")
  if(PURE_STUDIO_BRIDGE_LIBRARY STREQUAL "")
    message(FATAL_ERROR
      "PURE_STUDIO_BRIDGE_LIBRARY is required. Build or run the Windows GUI "
      "through 'cargo xtask build-gui' or 'cargo xtask run-gui'.")
  endif()
  if(NOT IS_ABSOLUTE "${PURE_STUDIO_BRIDGE_LIBRARY}")
    message(FATAL_ERROR
      "PURE_STUDIO_BRIDGE_LIBRARY must be an absolute path: "
      "${PURE_STUDIO_BRIDGE_LIBRARY}")
  endif()
  if(NOT EXISTS "${PURE_STUDIO_BRIDGE_LIBRARY}"
      OR IS_DIRECTORY "${PURE_STUDIO_BRIDGE_LIBRARY}")
    message(FATAL_ERROR
      "PURE_STUDIO_BRIDGE_LIBRARY is not a file: "
      "${PURE_STUDIO_BRIDGE_LIBRARY}")
  endif()

  set(PURE_STUDIO_BRIDGE_DLL
    "${CMAKE_BINARY_DIR}/runner/$<CONFIG>/pl_studio_bridge.dll")
  set(PURE_STUDIO_BRIDGE_OUTPUTS "${PURE_STUDIO_BRIDGE_DLL}")
  set(PURE_STUDIO_BRIDGE_DEPENDS "${PURE_STUDIO_BRIDGE_LIBRARY}")
  set(PURE_STUDIO_BRIDGE_COPY_DEBUG_SYMBOLS)

  set(PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS "$ENV{PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS}")
  if(NOT PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS STREQUAL "")
    if(NOT IS_ABSOLUTE "${PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS}")
      message(FATAL_ERROR
        "PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS must be an absolute path: "
        "${PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS}")
    endif()
    if(NOT EXISTS "${PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS}"
        OR IS_DIRECTORY "${PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS}")
      message(FATAL_ERROR
        "PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS is not a file: "
        "${PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS}")
    endif()

    set(PURE_STUDIO_BRIDGE_PDB
      "${CMAKE_BINARY_DIR}/runner/$<CONFIG>/pl_studio_bridge.pdb")
    list(APPEND PURE_STUDIO_BRIDGE_OUTPUTS "${PURE_STUDIO_BRIDGE_PDB}")
    list(APPEND PURE_STUDIO_BRIDGE_DEPENDS "${PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS}")
    list(APPEND PURE_STUDIO_BRIDGE_COPY_DEBUG_SYMBOLS
      COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "${PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS}" "${PURE_STUDIO_BRIDGE_PDB}")
  endif()

  # Flutter can reuse its native build graph when only the external Rust
  # workspace changed. Keep this target intentionally always out of date so
  # every invoked Windows build rechecks the prebuilt bridge bytes; the
  # copy_if_different commands avoid touching staged files unnecessarily.
  add_custom_target(pl_studio_bridge_dll
    COMMAND "${CMAKE_COMMAND}" -E make_directory
      "${CMAKE_BINARY_DIR}/runner/$<CONFIG>"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
      "${PURE_STUDIO_BRIDGE_LIBRARY}" "${PURE_STUDIO_BRIDGE_DLL}"
    ${PURE_STUDIO_BRIDGE_COPY_DEBUG_SYMBOLS}
    DEPENDS ${PURE_STUDIO_BRIDGE_DEPENDS}
    BYPRODUCTS ${PURE_STUDIO_BRIDGE_OUTPUTS}
    COMMENT "Staging prebuilt Rust bridge artifacts"
    VERBATIM
  )
  add_dependencies(${BINARY_NAME} pl_studio_bridge_dll)
endif()


# Generated plugin build rules, which manage building the plugins and adding
# them to the application.
include(flutter/generated_plugins.cmake)


# === Installation ===
# Support files are copied into place next to the executable, so that it can
# run in place. This is done instead of making a separate bundle (as on Linux)
# so that building and running from within Visual Studio will work.
set(BUILD_BUNDLE_DIR "$<TARGET_FILE_DIR:${BINARY_NAME}>")
# Make the "install" step default, as it's required to run.
set(CMAKE_VS_INCLUDE_INSTALL_TO_DEFAULT_BUILD 1)
if(CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT)
  set(CMAKE_INSTALL_PREFIX "${BUILD_BUNDLE_DIR}" CACHE PATH "..." FORCE)
endif()

set(INSTALL_BUNDLE_DATA_DIR "${CMAKE_INSTALL_PREFIX}/data")
set(INSTALL_BUNDLE_LIB_DIR "${CMAKE_INSTALL_PREFIX}")

install(TARGETS ${BINARY_NAME} RUNTIME DESTINATION "${CMAKE_INSTALL_PREFIX}"
  COMPONENT Runtime)

install(FILES "${FLUTTER_ICU_DATA_FILE}" DESTINATION "${INSTALL_BUNDLE_DATA_DIR}"
  COMPONENT Runtime)

install(FILES "${FLUTTER_LIBRARY}" DESTINATION "${INSTALL_BUNDLE_LIB_DIR}"
  COMPONENT Runtime)

if(PLUGIN_BUNDLED_LIBRARIES)
  install(FILES "${PLUGIN_BUNDLED_LIBRARIES}"
    DESTINATION "${INSTALL_BUNDLE_LIB_DIR}"
    COMPONENT Runtime)
endif()

# Copy the native assets provided by the build.dart from all packages.
set(NATIVE_ASSETS_DIR "${PROJECT_BUILD_DIR}native_assets/windows/")
install(DIRECTORY "${NATIVE_ASSETS_DIR}"
   DESTINATION "${INSTALL_BUNDLE_LIB_DIR}"
   COMPONENT Runtime
   OPTIONAL)

# Fully re-copy the assets directory on each build to avoid having stale files
# from a previous install.
set(FLUTTER_ASSET_DIR_NAME "flutter_assets")
install(CODE "
  file(REMOVE_RECURSE \"${INSTALL_BUNDLE_DATA_DIR}/${FLUTTER_ASSET_DIR_NAME}\")
  " COMPONENT Runtime)
install(DIRECTORY "${PROJECT_BUILD_DIR}/${FLUTTER_ASSET_DIR_NAME}"
  DESTINATION "${INSTALL_BUNDLE_DATA_DIR}" COMPONENT Runtime)

# Install the AOT library on non-Debug builds only.
install(FILES "${AOT_LIBRARY}" DESTINATION "${INSTALL_BUNDLE_DATA_DIR}"
  CONFIGURATIONS Profile;Release
  COMPONENT Runtime)
