function(sebwatch_add_test name)
    qt_add_executable(${name} ${name}.cpp)
    target_link_libraries(${name} PRIVATE sebwatch_core Qt6::Test)
    add_test(NAME ${name} COMMAND ${name})
endfunction()

sebwatch_add_test(tst_version)
target_compile_definitions(tst_version PRIVATE SEBWATCH_EXPECTED_VERSION="${PROJECT_VERSION}")
