qt_add_executable(sebwatch main.cpp mainwindow.cpp mainwindow.h)
target_link_libraries(sebwatch PRIVATE sebwatch_core Qt6::Widgets)
