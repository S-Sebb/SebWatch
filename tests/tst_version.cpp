#include "version.h"

#include <QTest>

// Proves the core library links into Qt Test executables; real tests arrive with the core.
class TestVersion : public QObject
{
    Q_OBJECT

private slots:
    void matchesProjectVersion() { QCOMPARE(sebwatch::version(), QStringLiteral("0.1.0")); }
};

QTEST_GUILESS_MAIN(TestVersion)
#include "tst_version.moc"
