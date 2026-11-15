cmake_minimum_required(VERSION 3.24)
project(SebWatch VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_COMPILE_WARNING_AS_ERROR ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
add_compile_options(-Wall -Wextra -Wpedantic)

# 6.4 is the oldest Qt on the CI image (Ubuntu 24.04).
find_package(Qt6 6.4 REQUIRED COMPONENTS Core Widgets Test)
qt_standard_project_setup()

enable_testing()

add_subdirectory(src/core)
add_subdirectory(src/app)
add_subdirectory(tests)
