#include "mainwindow.h"
#include "version.h"

#include <QApplication>

int main(int argc, char *argv[])
{
    // No style is set: Qt's platform theme picks the desktop's (Breeze on Plasma).
    QApplication app(argc, argv);
    QApplication::setApplicationName(QStringLiteral("SebWatch"));
    QApplication::setApplicationVersion(sebwatch::version());

    MainWindow window;
    window.show();
    return QApplication::exec();
}
