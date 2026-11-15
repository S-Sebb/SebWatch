# UI-free logic (accounting, readers). Links only Qt Core so tests can use it headless.
configure_file(version.h.in version.h @ONLY)
add_library(sebwatch_core STATIC version.cpp)
target_include_directories(sebwatch_core PUBLIC ${CMAKE_CURRENT_SOURCE_DIR} ${CMAKE_CURRENT_BINARY_DIR})
target_link_libraries(sebwatch_core PUBLIC Qt6::Core)
